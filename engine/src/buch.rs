//! Limit-Orderbuch mit Price-Time-Priority.
//!
//! Aufbau, und warum er so ist:
//!
//! * **Preise sind ganze Zahlen (Ticks), nie Fliesskomma.** Fliesskomma in
//!   einem Handelskern erzeugt Rundungsfehler, die als Geld verschwinden.
//! * **Preisstufen liegen in einem Feld, indiziert nach Tick.** Zugriff auf
//!   eine Stufe ist damit O(1) statt O(log n) wie bei einem Baum.
//! * **Die beste Seite wird über ein Bitfeld gesucht.** Ein gesetztes Bit je
//!   belegtem Tick; die nächste belegte Stufe findet man mit einer einzigen
//!   Bitoperation je 64 Ticks.
//! * **Orders einer Stufe hängen in einer verketteten Liste über Slot-Nummern**
//!   (kein `Box`, keine Zeiger). Einfügen und Stornieren sind O(1), und alle
//!   Knoten liegen zusammenhängend im Speicher.
//!
//! Ergebnis: Einfügen, Ausführen und Stornieren sind konstante Operationen.

pub const KEIN: u32 = u32::MAX;

/// Wie lange eine Order gilt.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gueltigkeit {
    /// Gilt, bis sie storniert wird. Rest bleibt im Buch liegen.
    Gtc,
    /// Sofort ausführen, Rest verfällt.
    Ioc,
    /// Darf nur ins Buch legen, nie nehmen. Würde sie kreuzen, wird sie
    /// abgelehnt - so kann ein Market Maker nie versehentlich Taker sein.
    NurLegen,
}

/// Eine Ausführung zwischen zwei Orders.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ausfuehrung {
    pub tick: u32,
    pub menge: u64,
    pub nehmer: u64,
    pub geber: u64,
    /// true, wenn der Nehmer gekauft hat.
    pub nehmer_kauft: bool,
}

/// Was aus einer eingereichten Order wurde.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ergebnis {
    /// Vergeben, sobald ein Rest im Buch liegt.
    pub oid: Option<u64>,
    pub ausgefuehrt: u64,
    pub ruhend: u64,
    pub abgelehnt: bool,
}

#[derive(Clone, Copy)]
struct Knoten {
    oid: u64,
    menge: u64,
    tick: u32,
    kauf: bool,
    weiter: u32,
    zurueck: u32,
}

#[derive(Clone, Copy)]
struct Stufe {
    kopf: u32,
    fuss: u32,
    summe: u64,
}

impl Stufe {
    const LEER: Stufe = Stufe { kopf: KEIN, fuss: KEIN, summe: 0 };
    #[inline(always)]
    fn ist_leer(&self) -> bool {
        self.kopf == KEIN
    }
}

// ---------------------------------------------------------------------------
/// Bitfeld über die Ticks: ein gesetztes Bit je belegter Preisstufe.
struct Bitfeld {
    woerter: Vec<u64>,
}

impl Bitfeld {
    fn neu(ticks: u32) -> Self {
        Bitfeld { woerter: vec![0u64; (ticks as usize + 63) / 64] }
    }

    #[inline(always)]
    fn setzen(&mut self, i: u32) {
        self.woerter[(i >> 6) as usize] |= 1u64 << (i & 63);
    }

    #[inline(always)]
    fn loeschen(&mut self, i: u32) {
        self.woerter[(i >> 6) as usize] &= !(1u64 << (i & 63));
    }

    /// Höchster belegter Tick, der nicht über `von` liegt.
    #[inline]
    fn hoechster_bis(&self, von: u32) -> Option<u32> {
        let mut w = (von >> 6) as usize;
        let bit = von & 63;
        // Im ersten Wort alles oberhalb von `von` ausblenden.
        let maske = if bit == 63 { u64::MAX } else { (1u64 << (bit + 1)) - 1 };
        let mut wert = self.woerter[w] & maske;
        loop {
            if wert != 0 {
                return Some((w as u32) * 64 + (63 - wert.leading_zeros()));
            }
            if w == 0 {
                return None;
            }
            w -= 1;
            wert = self.woerter[w];
        }
    }

    /// Tiefster belegter Tick, der nicht unter `von` liegt.
    #[inline]
    fn tiefster_ab(&self, von: u32) -> Option<u32> {
        let mut w = (von >> 6) as usize;
        if w >= self.woerter.len() {
            return None;
        }
        let bit = von & 63;
        let mut wert = self.woerter[w] & (u64::MAX << bit);
        loop {
            if wert != 0 {
                return Some((w as u32) * 64 + wert.trailing_zeros());
            }
            w += 1;
            if w >= self.woerter.len() {
                return None;
            }
            wert = self.woerter[w];
        }
    }
}

// ---------------------------------------------------------------------------
pub struct Orderbuch {
    ticks: u32,
    kauf_stufen: Vec<Stufe>,
    verk_stufen: Vec<Stufe>,
    kauf_belegt: Bitfeld,
    verk_belegt: Bitfeld,

    knoten: Vec<Knoten>,
    frei: Vec<u32>,
    /// Order-Nummer -> Slot. Die Börse vergibt die Nummern selbst und
    /// fortlaufend, darum genügt ein Feld statt einer Streutabelle.
    platz: Vec<u32>,
    naechste_oid: u64,

    bester_kauf: Option<u32>,
    bester_verkauf: Option<u32>,

    pub anzahl_ausfuehrungen: u64,
    pub volumen: u64,
    pub ruhende_orders: u64,
}

impl Orderbuch {
    /// `ticks` ist die Anzahl möglicher Preisstufen (0 .. ticks-1).
    pub fn neu(ticks: u32) -> Self {
        assert!(ticks > 0, "mindestens ein Tick");
        Orderbuch {
            ticks,
            kauf_stufen: vec![Stufe::LEER; ticks as usize],
            verk_stufen: vec![Stufe::LEER; ticks as usize],
            kauf_belegt: Bitfeld::neu(ticks),
            verk_belegt: Bitfeld::neu(ticks),
            knoten: Vec::with_capacity(1 << 16),
            frei: Vec::with_capacity(1 << 12),
            platz: Vec::with_capacity(1 << 16),
            naechste_oid: 1,
            bester_kauf: None,
            bester_verkauf: None,
            anzahl_ausfuehrungen: 0,
            volumen: 0,
            ruhende_orders: 0,
        }
    }

    #[inline(always)]
    pub fn bester_kauf(&self) -> Option<u32> {
        self.bester_kauf
    }
    #[inline(always)]
    pub fn bester_verkauf(&self) -> Option<u32> {
        self.bester_verkauf
    }

    /// Ruhende Menge auf einer Preisstufe.
    pub fn menge_auf(&self, kauf: bool, tick: u32) -> u64 {
        if tick >= self.ticks {
            return 0;
        }
        if kauf { self.kauf_stufen[tick as usize].summe } else { self.verk_stufen[tick as usize].summe }
    }

    pub fn spanne(&self) -> Option<u32> {
        match (self.bester_kauf, self.bester_verkauf) {
            (Some(k), Some(v)) => Some(v - k),
            _ => None,
        }
    }

    // -- Slots ------------------------------------------------------------
    #[inline(always)]
    fn slot_holen(&mut self, k: Knoten) -> u32 {
        match self.frei.pop() {
            Some(s) => {
                self.knoten[s as usize] = k;
                s
            }
            None => {
                self.knoten.push(k);
                (self.knoten.len() - 1) as u32
            }
        }
    }

    #[inline(always)]
    fn platz_setzen(&mut self, oid: u64, slot: u32) {
        let i = oid as usize;
        if i >= self.platz.len() {
            self.platz.resize(i + 1, KEIN);
        }
        self.platz[i] = slot;
    }

    // -- Order einreichen -------------------------------------------------
    /// Limit-Order. `aus` nimmt die entstandenen Ausführungen auf.
    pub fn limit(
        &mut self,
        kauf: bool,
        tick: u32,
        menge: u64,
        gueltigkeit: Gueltigkeit,
        aus: &mut Vec<Ausfuehrung>,
    ) -> Ergebnis {
        if menge == 0 || tick >= self.ticks {
            return Ergebnis { oid: None, ausgefuehrt: 0, ruhend: 0, abgelehnt: true };
        }

        // Nur-Legen: würde sie kreuzen, wird sie gar nicht erst angenommen.
        if gueltigkeit == Gueltigkeit::NurLegen && self.wuerde_kreuzen(kauf, tick) {
            return Ergebnis { oid: None, ausgefuehrt: 0, ruhend: 0, abgelehnt: true };
        }

        let nehmer_oid = self.naechste_oid;
        self.naechste_oid += 1;

        let rest = self.ausfuehren(kauf, tick, menge, nehmer_oid, aus);
        let ausgefuehrt = menge - rest;

        if rest == 0 || gueltigkeit == Gueltigkeit::Ioc {
            return Ergebnis { oid: None, ausgefuehrt, ruhend: 0, abgelehnt: false };
        }

        self.legen(nehmer_oid, kauf, tick, rest);
        Ergebnis { oid: Some(nehmer_oid), ausgefuehrt, ruhend: rest, abgelehnt: false }
    }

    /// Market-Order: nimmt, was da ist, zu jedem Preis.
    pub fn markt(&mut self, kauf: bool, menge: u64, aus: &mut Vec<Ausfuehrung>) -> Ergebnis {
        if menge == 0 {
            return Ergebnis { oid: None, ausgefuehrt: 0, ruhend: 0, abgelehnt: true };
        }
        let oid = self.naechste_oid;
        self.naechste_oid += 1;
        let grenze = if kauf { self.ticks - 1 } else { 0 };
        let rest = self.ausfuehren(kauf, grenze, menge, oid, aus);
        Ergebnis { oid: None, ausgefuehrt: menge - rest, ruhend: 0, abgelehnt: false }
    }

    #[inline(always)]
    fn wuerde_kreuzen(&self, kauf: bool, tick: u32) -> bool {
        if kauf {
            matches!(self.bester_verkauf, Some(v) if tick >= v)
        } else {
            matches!(self.bester_kauf, Some(k) if tick <= k)
        }
    }

    /// Kern: gegen die Gegenseite abarbeiten. Gibt den Rest zurück.
    fn ausfuehren(
        &mut self,
        kauf: bool,
        grenze: u32,
        mut menge: u64,
        nehmer_oid: u64,
        aus: &mut Vec<Ausfuehrung>,
    ) -> u64 {
        while menge > 0 {
            let stufe_tick = if kauf {
                match self.bester_verkauf {
                    Some(v) if v <= grenze => v,
                    _ => break,
                }
            } else {
                match self.bester_kauf {
                    Some(k) if k >= grenze => k,
                    _ => break,
                }
            };

            // Preis-Zeit-Vorrang: immer der älteste Knoten der Stufe zuerst.
            loop {
                if menge == 0 {
                    break;
                }
                let stufe = if kauf {
                    &mut self.verk_stufen[stufe_tick as usize]
                } else {
                    &mut self.kauf_stufen[stufe_tick as usize]
                };
                let kopf = stufe.kopf;
                if kopf == KEIN {
                    break;
                }

                let geber_menge = self.knoten[kopf as usize].menge;
                let geber_oid = self.knoten[kopf as usize].oid;
                let m = if menge < geber_menge { menge } else { geber_menge };

                menge -= m;
                self.knoten[kopf as usize].menge -= m;
                stufe.summe -= m;

                aus.push(Ausfuehrung {
                    tick: stufe_tick,
                    menge: m,
                    nehmer: nehmer_oid,
                    geber: geber_oid,
                    nehmer_kauft: kauf,
                });
                self.anzahl_ausfuehrungen += 1;
                self.volumen += m;

                if self.knoten[kopf as usize].menge == 0 {
                    self.kopf_entfernen(kauf, stufe_tick);
                }
            }

            // Stufe leergeräumt? Dann Bit löschen und nächste beste suchen.
            let leer = if kauf {
                self.verk_stufen[stufe_tick as usize].ist_leer()
            } else {
                self.kauf_stufen[stufe_tick as usize].ist_leer()
            };
            if leer {
                if kauf {
                    self.verk_belegt.loeschen(stufe_tick);
                    self.bester_verkauf = if stufe_tick + 1 < self.ticks {
                        self.verk_belegt.tiefster_ab(stufe_tick + 1)
                    } else {
                        None
                    };
                } else {
                    self.kauf_belegt.loeschen(stufe_tick);
                    self.bester_kauf = if stufe_tick > 0 {
                        self.kauf_belegt.hoechster_bis(stufe_tick - 1)
                    } else {
                        None
                    };
                }
            } else if menge == 0 {
                break;
            }
        }
        menge
    }

    /// Ältesten Knoten einer Stufe aushängen (er ist vollständig ausgeführt).
    #[inline]
    fn kopf_entfernen(&mut self, gegenseite_kauf: bool, tick: u32) {
        let stufe = if gegenseite_kauf {
            &mut self.verk_stufen[tick as usize]
        } else {
            &mut self.kauf_stufen[tick as usize]
        };
        let kopf = stufe.kopf;
        let weiter = self.knoten[kopf as usize].weiter;
        stufe.kopf = weiter;
        if weiter == KEIN {
            stufe.fuss = KEIN;
        } else {
            self.knoten[weiter as usize].zurueck = KEIN;
        }
        let oid = self.knoten[kopf as usize].oid;
        self.platz[oid as usize] = KEIN;
        self.frei.push(kopf);
        self.ruhende_orders -= 1;
    }

    /// Rest ins Buch legen.
    fn legen(&mut self, oid: u64, kauf: bool, tick: u32, menge: u64) {
        let slot = self.slot_holen(Knoten {
            oid,
            menge,
            tick,
            kauf,
            weiter: KEIN,
            zurueck: KEIN,
        });
        self.platz_setzen(oid, slot);

        let stufe = if kauf {
            &mut self.kauf_stufen[tick as usize]
        } else {
            &mut self.verk_stufen[tick as usize]
        };
        let war_leer = stufe.ist_leer();
        if war_leer {
            stufe.kopf = slot;
            stufe.fuss = slot;
        } else {
            let fuss = stufe.fuss;
            self.knoten[fuss as usize].weiter = slot;
            self.knoten[slot as usize].zurueck = fuss;
            let stufe = if kauf {
                &mut self.kauf_stufen[tick as usize]
            } else {
                &mut self.verk_stufen[tick as usize]
            };
            stufe.fuss = slot;
        }
        let stufe = if kauf {
            &mut self.kauf_stufen[tick as usize]
        } else {
            &mut self.verk_stufen[tick as usize]
        };
        stufe.summe += menge;
        self.ruhende_orders += 1;

        if war_leer {
            if kauf {
                self.kauf_belegt.setzen(tick);
                if self.bester_kauf.map_or(true, |b| tick > b) {
                    self.bester_kauf = Some(tick);
                }
            } else {
                self.verk_belegt.setzen(tick);
                if self.bester_verkauf.map_or(true, |b| tick < b) {
                    self.bester_verkauf = Some(tick);
                }
            }
        }
    }

    // -- Stornieren -------------------------------------------------------
    /// Gibt die stornierte Restmenge zurück, oder None, wenn es die Order
    /// nicht (mehr) gibt.
    pub fn stornieren(&mut self, oid: u64) -> Option<u64> {
        let i = oid as usize;
        if i >= self.platz.len() {
            return None;
        }
        let slot = self.platz[i];
        if slot == KEIN {
            return None;
        }

        let k = self.knoten[slot as usize];
        let (tick, kauf, menge) = (k.tick, k.kauf, k.menge);

        // Aus der verketteten Liste aushängen.
        if k.zurueck != KEIN {
            self.knoten[k.zurueck as usize].weiter = k.weiter;
        }
        if k.weiter != KEIN {
            self.knoten[k.weiter as usize].zurueck = k.zurueck;
        }
        let stufe = if kauf {
            &mut self.kauf_stufen[tick as usize]
        } else {
            &mut self.verk_stufen[tick as usize]
        };
        if stufe.kopf == slot {
            stufe.kopf = k.weiter;
        }
        if stufe.fuss == slot {
            stufe.fuss = k.zurueck;
        }
        stufe.summe -= menge;
        let jetzt_leer = stufe.ist_leer();

        self.platz[i] = KEIN;
        self.frei.push(slot);
        self.ruhende_orders -= 1;

        if jetzt_leer {
            if kauf {
                self.kauf_belegt.loeschen(tick);
                if self.bester_kauf == Some(tick) {
                    self.bester_kauf =
                        if tick > 0 { self.kauf_belegt.hoechster_bis(tick - 1) } else { None };
                }
            } else {
                self.verk_belegt.loeschen(tick);
                if self.bester_verkauf == Some(tick) {
                    self.bester_verkauf = if tick + 1 < self.ticks {
                        self.verk_belegt.tiefster_ab(tick + 1)
                    } else {
                        None
                    };
                }
            }
        }
        Some(menge)
    }

    /// Liegt diese Order noch im Buch?
    pub fn liegt(&self, oid: u64) -> bool {
        let i = oid as usize;
        i < self.platz.len() && self.platz[i] != KEIN
    }
}

// ===========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn buch() -> (Orderbuch, Vec<Ausfuehrung>) {
        (Orderbuch::neu(4096), Vec::new())
    }

    #[test]
    fn legen_und_beste_preise() {
        let (mut b, mut a) = buch();
        b.limit(true, 100, 10, Gueltigkeit::Gtc, &mut a);
        b.limit(true, 102, 10, Gueltigkeit::Gtc, &mut a);
        b.limit(false, 110, 10, Gueltigkeit::Gtc, &mut a);
        b.limit(false, 108, 10, Gueltigkeit::Gtc, &mut a);
        assert_eq!(b.bester_kauf(), Some(102));
        assert_eq!(b.bester_verkauf(), Some(108));
        assert_eq!(b.spanne(), Some(6));
        assert!(a.is_empty(), "nichts darf gekreuzt haben");
    }

    #[test]
    fn zeitvorrang_bei_gleichem_preis() {
        let (mut b, mut a) = buch();
        let e1 = b.limit(false, 100, 5, Gueltigkeit::Gtc, &mut a);
        let e2 = b.limit(false, 100, 5, Gueltigkeit::Gtc, &mut a);
        a.clear();
        b.limit(true, 100, 5, Gueltigkeit::Gtc, &mut a);
        assert_eq!(a.len(), 1);
        // Die zuerst gelegte Order muss zuerst bedient werden.
        assert_eq!(a[0].geber, e1.oid.unwrap());
        assert!(b.liegt(e2.oid.unwrap()));
        assert!(!b.liegt(e1.oid.unwrap()));
    }

    #[test]
    fn teilausfuehrung() {
        let (mut b, mut a) = buch();
        let e = b.limit(false, 100, 10, Gueltigkeit::Gtc, &mut a);
        a.clear();
        let k = b.limit(true, 100, 4, Gueltigkeit::Gtc, &mut a);
        assert_eq!(k.ausgefuehrt, 4);
        assert_eq!(k.ruhend, 0);
        assert_eq!(b.menge_auf(false, 100), 6);
        assert!(b.liegt(e.oid.unwrap()));
    }

    #[test]
    fn ueber_mehrere_stufen() {
        let (mut b, mut a) = buch();
        b.limit(false, 100, 5, Gueltigkeit::Gtc, &mut a);
        b.limit(false, 101, 5, Gueltigkeit::Gtc, &mut a);
        b.limit(false, 103, 5, Gueltigkeit::Gtc, &mut a);
        a.clear();
        let e = b.limit(true, 102, 12, Gueltigkeit::Gtc, &mut a);
        assert_eq!(e.ausgefuehrt, 10, "nur bis Tick 102 darf genommen werden");
        assert_eq!(e.ruhend, 2);
        assert_eq!(a.len(), 2);
        assert_eq!(b.bester_verkauf(), Some(103));
        assert_eq!(b.bester_kauf(), Some(102));
    }

    #[test]
    fn ioc_verfaellt() {
        let (mut b, mut a) = buch();
        b.limit(false, 100, 3, Gueltigkeit::Gtc, &mut a);
        a.clear();
        let e = b.limit(true, 100, 10, Gueltigkeit::Ioc, &mut a);
        assert_eq!(e.ausgefuehrt, 3);
        assert_eq!(e.ruhend, 0);
        assert_eq!(b.bester_kauf(), None, "IOC darf nichts liegen lassen");
    }

    #[test]
    fn nur_legen_wird_abgelehnt() {
        let (mut b, mut a) = buch();
        b.limit(false, 100, 5, Gueltigkeit::Gtc, &mut a);
        let e = b.limit(true, 100, 5, Gueltigkeit::NurLegen, &mut a);
        assert!(e.abgelehnt);
        assert_eq!(b.menge_auf(false, 100), 5);
        // Knapp darunter ist erlaubt.
        let e2 = b.limit(true, 99, 5, Gueltigkeit::NurLegen, &mut a);
        assert!(!e2.abgelehnt);
        assert_eq!(b.bester_kauf(), Some(99));
    }

    #[test]
    fn stornieren_setzt_beste_preise_zurueck() {
        let (mut b, mut a) = buch();
        let e1 = b.limit(true, 100, 5, Gueltigkeit::Gtc, &mut a);
        let e2 = b.limit(true, 105, 5, Gueltigkeit::Gtc, &mut a);
        assert_eq!(b.bester_kauf(), Some(105));
        assert_eq!(b.stornieren(e2.oid.unwrap()), Some(5));
        assert_eq!(b.bester_kauf(), Some(100));
        assert_eq!(b.stornieren(e1.oid.unwrap()), Some(5));
        assert_eq!(b.bester_kauf(), None);
        assert_eq!(b.stornieren(e1.oid.unwrap()), None, "zweimal geht nicht");
    }

    #[test]
    fn stornieren_aus_der_mitte() {
        let (mut b, mut a) = buch();
        let e1 = b.limit(false, 100, 1, Gueltigkeit::Gtc, &mut a);
        let e2 = b.limit(false, 100, 2, Gueltigkeit::Gtc, &mut a);
        let e3 = b.limit(false, 100, 3, Gueltigkeit::Gtc, &mut a);
        b.stornieren(e2.oid.unwrap());
        assert_eq!(b.menge_auf(false, 100), 4);
        a.clear();
        b.limit(true, 100, 4, Gueltigkeit::Gtc, &mut a);
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].geber, e1.oid.unwrap());
        assert_eq!(a[1].geber, e3.oid.unwrap());
    }

    #[test]
    fn markt_order_raeumt_ab() {
        let (mut b, mut a) = buch();
        b.limit(false, 100, 5, Gueltigkeit::Gtc, &mut a);
        b.limit(false, 101, 5, Gueltigkeit::Gtc, &mut a);
        a.clear();
        let e = b.markt(true, 8, &mut a);
        assert_eq!(e.ausgefuehrt, 8);
        assert_eq!(b.menge_auf(false, 101), 2);
        assert_eq!(b.bester_verkauf(), Some(101));
    }

    #[test]
    fn buch_bleibt_bilanziert() {
        // Zufälliger Ablauf: die Summe aller Ausführungen muss dem
        // gemeldeten Volumen entsprechen, und ruhende Orders müssen stimmen.
        let mut b = Orderbuch::neu(1024);
        let mut a = Vec::new();
        let mut zustand = 12345u64;
        let mut wuerfel = || {
            zustand ^= zustand << 13;
            zustand ^= zustand >> 7;
            zustand ^= zustand << 17;
            zustand
        };
        let mut liegend: Vec<u64> = Vec::new();
        let mut summe_ausfuehrungen = 0u64;
        for _ in 0..50_000 {
            let r = wuerfel();
            if r % 5 == 0 && !liegend.is_empty() {
                let i = (wuerfel() % liegend.len() as u64) as usize;
                let oid = liegend.swap_remove(i);
                b.stornieren(oid);
            } else {
                let kauf = r % 2 == 0;
                let tick = 400 + (wuerfel() % 200) as u32;
                let menge = 1 + wuerfel() % 20;
                a.clear();
                let e = b.limit(kauf, tick, menge, Gueltigkeit::Gtc, &mut a);
                for x in a.iter() {
                    summe_ausfuehrungen += x.menge;
                }
                if let Some(oid) = e.oid {
                    liegend.push(oid);
                }
            }
        }
        assert_eq!(summe_ausfuehrungen, b.volumen);
        let tatsaechlich_liegend = liegend.iter().filter(|o| b.liegt(**o)).count() as u64;
        assert_eq!(tatsaechlich_liegend, b.ruhende_orders);
        // Das Buch darf nie gekreuzt sein.
        if let (Some(k), Some(v)) = (b.bester_kauf(), b.bester_verkauf()) {
            assert!(k < v, "gekreuztes Buch: {k} >= {v}");
        }
    }
}
