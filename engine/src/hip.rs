//! HIP-1 und HIP-2 nachgebaut.
//!
//! Hyperliquid hat zwei Bausteine, die zusammen einen Token erst handelbar
//! machen. Beide sind gut dokumentiert und liessen sich hier nachbauen:
//!
//! * **HIP-1** ist der Token-Standard: Name, Dezimalen, Höchstmenge,
//!   Anfangsverteilung. Jeder so eingerichtete Token bekommt automatisch ein
//!   eigenes Spot-Orderbuch gegen USDC. Der Platz dafür wird in einer
//!   holländischen Auktion versteigert.
//! * **HIP-2 «Hyperliquidity»** ist der Teil, den fast alle übersehen: eine
//!   Leiter aus Orders, die das Protokoll selbst stellt. Sie garantiert eine
//!   Spanne von 0.3 % und braucht keinen Market Maker.
//!
//! Der zweite Punkt ist der eigentliche Trick. Ein Token ohne Liquidität ist
//! ein leeres Orderbuch - HIP-2 löst genau das, ohne Vertrauen in einen
//! Dritten.

use crate::buch::{Ausfuehrung, Gueltigkeit, Orderbuch};

// ===========================================================================
// HIP-1 - Token-Standard
// ===========================================================================

#[derive(Clone, Debug)]
pub struct TokenBeschrieb {
    /// Höchstens 6 Zeichen, muss nicht eindeutig sein.
    pub name: String,
    /// Dezimalen der kleinsten Einheit (ETH hätte 18).
    pub wei_dezimalen: u8,
    /// Handelbare Dezimalen im Orderbuch. Bedingung: sz + 5 <= wei.
    pub sz_dezimalen: u8,
    /// Anfangs- und Höchstmenge in kleinsten Einheiten.
    pub max_menge_wei: u128,
}

#[derive(Debug, PartialEq)]
pub enum Beanstandung {
    NameZuLang(usize),
    NameLeer,
    DezimalenAbstand { wei: u8, sz: u8 },
    MengeNull,
    MengeNichtDurchLos,
}

impl TokenBeschrieb {
    /// Losgrösse: die kleinste im Buch handelbare Menge.
    pub fn los_groesse(&self) -> u128 {
        10u128.pow((self.wei_dezimalen - self.sz_dezimalen) as u32)
    }

    /// Prüft die Regeln von HIP-1, bevor irgendetwas Geld kostet.
    pub fn pruefen(&self) -> Result<(), Vec<Beanstandung>> {
        let mut fehler = Vec::new();
        let laenge = self.name.chars().count();
        if laenge == 0 {
            fehler.push(Beanstandung::NameLeer);
        } else if laenge > 6 {
            fehler.push(Beanstandung::NameZuLang(laenge));
        }
        if self.sz_dezimalen + 5 > self.wei_dezimalen {
            fehler.push(Beanstandung::DezimalenAbstand {
                wei: self.wei_dezimalen,
                sz: self.sz_dezimalen,
            });
        }
        if self.max_menge_wei == 0 {
            fehler.push(Beanstandung::MengeNull);
        } else if self.sz_dezimalen + 5 <= self.wei_dezimalen
            && self.max_menge_wei % self.los_groesse() != 0
        {
            fehler.push(Beanstandung::MengeNichtDurchLos);
        }
        if fehler.is_empty() { Ok(()) } else { Err(fehler) }
    }

    /// Höchstmenge in ganzen Token, zur Kontrolle beim Einrichten.
    pub fn max_menge_ganz(&self) -> f64 {
        self.max_menge_wei as f64 / 10f64.powi(self.wei_dezimalen as i32)
    }
}

// ===========================================================================
// Holländische Auktion um den Platz im Buch
// ===========================================================================

/// Bei Hyperliquid: 31 Stunden, linear fallend, Endpreis 500 HYPE.
/// Startpreis ist das Doppelte der letzten erfolgreichen Auktion - oder
/// wieder 500, wenn die letzte niemand genommen hat.
#[derive(Clone, Copy, Debug)]
pub struct Auktion {
    pub start_preis: f64,
    pub end_preis: f64,
    pub dauer_sekunden: u64,
}

impl Auktion {
    pub const DAUER_HYPERLIQUID: u64 = 31 * 3600;
    pub const BODEN_HYPERLIQUID: f64 = 500.0;

    pub fn naechste(letzter_zuschlag: Option<f64>) -> Self {
        Auktion {
            start_preis: match letzter_zuschlag {
                Some(p) => (2.0 * p).max(Self::BODEN_HYPERLIQUID),
                None => Self::BODEN_HYPERLIQUID,
            },
            end_preis: Self::BODEN_HYPERLIQUID,
            dauer_sekunden: Self::DAUER_HYPERLIQUID,
        }
    }

    /// Preis nach `sekunden` Laufzeit.
    pub fn preis_bei(&self, sekunden: u64) -> f64 {
        if sekunden >= self.dauer_sekunden {
            return self.end_preis;
        }
        let anteil = sekunden as f64 / self.dauer_sekunden as f64;
        self.start_preis - (self.start_preis - self.end_preis) * anteil
    }

    /// Wann fällt der Preis erstmals auf höchstens `budget`?
    pub fn wartezeit_bis(&self, budget: f64) -> Option<u64> {
        if budget >= self.start_preis {
            return Some(0);
        }
        if budget < self.end_preis {
            return None;
        }
        let spanne = self.start_preis - self.end_preis;
        if spanne <= 0.0 {
            return Some(0);
        }
        let anteil = (self.start_preis - budget) / spanne;
        Some((anteil * self.dauer_sekunden as f64).ceil() as u64)
    }
}

// ===========================================================================
// HIP-2 - Hyperliquidity
// ===========================================================================

/// Schrittweite der Leiter: jede Stufe liegt 0.3 % über der darunter.
pub const SCHRITT_PROMILLE: u64 = 1003;

/// Mindestabstand zwischen zwei Aktualisierungen.
pub const TAKT_SEKUNDEN: u64 = 3;

/// Die Orderleiter, die das Protokoll selbst stellt.
///
/// Unter einer Trennlinie stehen Kaufaufträge, darüber Verkaufsaufträge.
/// Wird unten gekauft, wandert die Trennlinie nach unten und die freigewordene
/// Stufe wird zum Verkaufsauftrag - und umgekehrt. Genau daraus entsteht die
/// garantierte Spanne, ohne dass jemand eingreift.
pub struct Hyperliquiditaet {
    pub preise: Vec<u64>,
    pub order_groesse: u64,
    /// Token-Bestand des Protokolls, in Losen.
    pub basis_bestand: u64,
    /// Quote-Bestand (USDC) in Preis-mal-Menge-Einheiten.
    pub quote_bestand: u128,
    /// Order-Nummern je Stufe, solange sie im Buch liegen.
    liegend: Vec<Option<u64>>,
    /// true = diese Stufe ist derzeit ein Kaufauftrag.
    ist_kauf: Vec<bool>,
    pub letzte_aktualisierung: u64,
    pub aktualisierungen: u64,
}

impl Hyperliquiditaet {
    /// `n_gesaet` Stufen starten als Kaufaufträge, der Rest als Verkaufsaufträge.
    pub fn neu(start_preis: u64, n_orders: u32, order_groesse: u64, n_gesaet: u32) -> Self {
        assert!(n_orders > 0 && order_groesse > 0);
        assert!(n_gesaet <= n_orders, "mehr gesäte Stufen als Stufen");

        let mut preise = Vec::with_capacity(n_orders as usize);
        let mut p = start_preis;
        for i in 0..n_orders {
            if i > 0 {
                // round(p * 1.003) in Ganzzahlen - kein Fliesskomma im Kern.
                p = (p * SCHRITT_PROMILLE + 500) / 1000;
                if p <= preise[(i - 1) as usize] {
                    p = preise[(i - 1) as usize] + 1; // bei winzigen Preisen
                }
            }
            preise.push(p);
        }

        let n = n_orders as usize;
        Hyperliquiditaet {
            preise,
            order_groesse,
            // Token für alle Verkaufsstufen, USDC für alle Kaufstufen.
            basis_bestand: (n_orders - n_gesaet) as u64 * order_groesse,
            quote_bestand: 0,
            liegend: vec![None; n],
            ist_kauf: (0..n).map(|i| (i as u32) < n_gesaet).collect(),
            letzte_aktualisierung: 0,
            aktualisierungen: 0,
        }
    }

    /// Garantierte Spanne in Basispunkten: 0.3 % = 30 bp.
    pub fn spanne_bp(&self) -> u64 {
        (SCHRITT_PROMILLE - 1000) * 10
    }

    pub fn hoechster_kauf(&self) -> Option<u64> {
        self.ist_kauf
            .iter()
            .enumerate()
            .filter(|(_, k)| **k)
            .map(|(i, _)| self.preise[i])
            .max()
    }

    pub fn tiefster_verkauf(&self) -> Option<u64> {
        self.ist_kauf
            .iter()
            .enumerate()
            .filter(|(_, k)| !**k)
            .map(|(i, _)| self.preise[i])
            .min()
    }

    /// Alle Stufen ins Buch legen. Wird beim Start einmal aufgerufen.
    pub fn stellen(&mut self, buch: &mut Orderbuch, aus: &mut Vec<Ausfuehrung>) {
        for i in 0..self.preise.len() {
            self.stufe_stellen(i, buch, aus);
        }
    }

    fn stufe_stellen(&mut self, i: usize, buch: &mut Orderbuch, aus: &mut Vec<Ausfuehrung>) {
        if self.liegend[i].is_some() {
            return;
        }
        let preis = self.preise[i];
        let kauf = self.ist_kauf[i];
        // Nur legen, nie nehmen: Das Protokoll ist immer Market Maker.
        let e = buch.limit(kauf, preis as u32, self.order_groesse, Gueltigkeit::NurLegen, aus);
        self.liegend[i] = e.oid;
    }

    fn stufe_ziehen(&mut self, i: usize, buch: &mut Orderbuch) {
        if let Some(oid) = self.liegend[i].take() {
            buch.stornieren(oid);
        }
    }

    /// Eine Ausführung verbuchen, die eine unserer Stufen getroffen hat.
    pub fn ausfuehrung_verbuchen(&mut self, a: &Ausfuehrung) -> bool {
        let Some(i) = self.liegend.iter().position(|o| *o == Some(a.geber)) else {
            return false;
        };
        if self.ist_kauf[i] {
            // Wir haben gekauft: Token rein, USDC raus.
            self.basis_bestand += a.menge;
            let kosten = a.menge as u128 * self.preise[i] as u128;
            self.quote_bestand = self.quote_bestand.saturating_sub(kosten);
        } else {
            // Wir haben verkauft: Token raus, USDC rein.
            self.basis_bestand = self.basis_bestand.saturating_sub(a.menge);
            self.quote_bestand += a.menge as u128 * self.preise[i] as u128;
        }
        true
    }

    /// Alle 3 Sekunden: Stufen neu verteilen und wieder ins Buch legen.
    ///
    /// nFull = floor(Bestand / Ordergrösse) bestimmt, wie viele Stufen von
    /// oben als Verkaufsaufträge gestellt werden können; der Rest wird zu
    /// Kaufaufträgen.
    pub fn aktualisieren(
        &mut self,
        zeit: u64,
        buch: &mut Orderbuch,
        aus: &mut Vec<Ausfuehrung>,
    ) -> bool {
        if self.aktualisierungen > 0 && zeit < self.letzte_aktualisierung + TAKT_SEKUNDEN {
            return false;
        }
        self.letzte_aktualisierung = zeit;
        self.aktualisierungen += 1;

        let n = self.preise.len();
        let voll = (self.basis_bestand / self.order_groesse) as usize;
        let verkauf_stufen = voll.min(n);
        // Die obersten `verkauf_stufen` Stufen sind Verkaufsaufträge,
        // alles darunter sind Kaufaufträge.
        let grenze = n - verkauf_stufen;

        for i in 0..n {
            let soll_kauf = i < grenze;
            // Ausgeführte Stufen liegen nicht mehr im Buch und müssen neu.
            let fehlt = self.liegend[i].is_none();
            if self.ist_kauf[i] != soll_kauf || fehlt {
                self.stufe_ziehen(i, buch);
                self.ist_kauf[i] = soll_kauf;
                self.stufe_stellen(i, buch, aus);
            }
        }
        true
    }
}

// ===========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hip1_regeln() {
        let gut = TokenBeschrieb {
            name: "ARCH".into(),
            wei_dezimalen: 9,
            sz_dezimalen: 2,
            max_menge_wei: 1_000_000 * 10u128.pow(9),
        };
        assert!(gut.pruefen().is_ok());
        assert_eq!(gut.los_groesse(), 10u128.pow(7));
        assert_eq!(gut.max_menge_ganz(), 1_000_000.0);

        let zu_lang = TokenBeschrieb { name: "ARCHTUM".into(), ..gut.clone() };
        assert!(zu_lang.pruefen().unwrap_err().contains(&Beanstandung::NameZuLang(7)));

        let eng = TokenBeschrieb { wei_dezimalen: 6, sz_dezimalen: 2, ..gut.clone() };
        assert!(eng
            .pruefen()
            .unwrap_err()
            .contains(&Beanstandung::DezimalenAbstand { wei: 6, sz: 2 }));
    }

    #[test]
    fn auktion_faellt_linear() {
        let a = Auktion::naechste(Some(4000.0));
        assert_eq!(a.start_preis, 8000.0);
        assert_eq!(a.preis_bei(0), 8000.0);
        assert_eq!(a.preis_bei(Auktion::DAUER_HYPERLIQUID), 500.0);
        let mitte = a.preis_bei(Auktion::DAUER_HYPERLIQUID / 2);
        assert!((mitte - 4250.0).abs() < 1.0, "Mitte war {mitte}");

        // Nach einer gescheiterten Auktion beginnt es wieder beim Boden.
        assert_eq!(Auktion::naechste(None).start_preis, 500.0);

        // Wartezeit bis zum eigenen Budget
        let w = a.wartezeit_bis(2000.0).unwrap();
        assert!(a.preis_bei(w) <= 2000.0);
        assert!(a.wartezeit_bis(499.0).is_none(), "unter dem Boden nie");
    }

    #[test]
    fn leiter_steigt_um_drei_promille() {
        let h = Hyperliquiditaet::neu(10_000, 5, 100, 2);
        assert_eq!(h.preise[0], 10_000);
        assert_eq!(h.preise[1], 10_030);
        assert_eq!(h.preise[2], 10_060);
        assert_eq!(h.spanne_bp(), 30);
        // Zwei Kaufstufen unten, drei Verkaufsstufen darüber.
        assert_eq!(h.hoechster_kauf(), Some(10_030));
        assert_eq!(h.tiefster_verkauf(), Some(10_060));
    }

    #[test]
    fn leiter_stellt_beidseitig_und_kreuzt_nicht() {
        let mut buch = Orderbuch::neu(65_536);
        let mut aus = Vec::new();
        let mut h = Hyperliquiditaet::neu(10_000, 20, 100, 10);
        h.stellen(&mut buch, &mut aus);
        assert!(aus.is_empty(), "beim Stellen darf nichts kreuzen");
        assert_eq!(buch.ruhende_orders, 20);
        let k = buch.bester_kauf().unwrap();
        let v = buch.bester_verkauf().unwrap();
        assert!(k < v);
        // Spanne rund 0.3 %
        let bp = (v - k) as u64 * 10_000 / k as u64;
        assert!((25..=35).contains(&bp), "Spanne war {bp} bp");
    }

    #[test]
    fn leiter_wandert_nach_kauf() {
        let mut buch = Orderbuch::neu(65_536);
        let mut aus = Vec::new();
        let mut h = Hyperliquiditaet::neu(10_000, 20, 100, 10);
        h.stellen(&mut buch, &mut aus);
        h.aktualisieren(0, &mut buch, &mut aus);

        let bestand_vorher = h.basis_bestand;
        let verkauf_vorher = buch.bester_verkauf().unwrap();

        // Jemand kauft die unterste Verkaufsstufe komplett weg.
        aus.clear();
        buch.markt(true, 100, &mut aus);
        assert_eq!(aus.len(), 1);
        for a in aus.clone().iter() {
            assert!(h.ausfuehrung_verbuchen(a), "Stufe muss erkannt werden");
        }
        assert_eq!(h.basis_bestand, bestand_vorher - 100, "Token abgegeben");
        assert!(h.quote_bestand > 0, "USDC eingenommen");

        // Zu früh: der Takt von 3 Sekunden gilt.
        assert!(!h.aktualisieren(1, &mut buch, &mut aus));
        // Nach dem Takt wird die Leiter neu verteilt.
        aus.clear();
        assert!(h.aktualisieren(3, &mut buch, &mut aus));
        assert_eq!(buch.ruhende_orders, 20, "die Leiter ist wieder vollständig");
        assert!(
            buch.bester_verkauf().unwrap() > verkauf_vorher,
            "die Verkaufsseite ist nach oben gewandert"
        );
        assert!(buch.bester_kauf().unwrap() < buch.bester_verkauf().unwrap());
    }
}
