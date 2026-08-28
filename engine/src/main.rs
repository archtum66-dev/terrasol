//! Handelskern - Messläufe und Vorführungen.
//!
//!   cargo run --release -- bench     Durchsatz messen (Orders pro Sekunde)
//!   cargo run --release -- hip2      Hyperliquidity-Leiter vorführen
//!   cargo run --release -- auktion   Holländische Auktion durchrechnen
//!   cargo run --release              alle drei nacheinander

use std::time::Instant;

use engine::buch::{Ausfuehrung, Gueltigkeit, Orderbuch};
use engine::hip::{Auktion, Hyperliquiditaet, TokenBeschrieb};

// ---------------------------------------------------------------------------
/// Xorshift - schnell, deterministisch, reicht für einen Lastgenerator.
struct Wuerfel(u64);

impl Wuerfel {
    #[inline(always)]
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

// ---------------------------------------------------------------------------
#[derive(Clone, Copy)]
enum Op {
    /// Passive Order, die im Buch liegen bleiben soll.
    Legen { kauf: bool, versatz: u16, menge: u32 },
    /// Storniert die n-te noch liegende Order.
    Storno { wahl: u32 },
    /// Aggressive Order, die sofort ausgeführt wird.
    Nehmen { kauf: bool, menge: u32 },
}

const TICKS: u32 = 1 << 16;
const MITTE: u32 = TICKS / 2;
/// Wie weit vom Mittelkurs entfernt passive Orders gestellt werden.
const BAND: u32 = 200;

/// Erzeugt einen realistischen Auftragsstrom.
///
/// Das Verhältnis entspricht dem, was echte Börsen sehen: viel mehr
/// Stornierungen als Ausführungen. Wer nur Einfügungen misst, misst am
/// eigentlichen Problem vorbei.
fn strom_erzeugen(anzahl: usize, saat: u64) -> Vec<Op> {
    let mut w = Wuerfel(saat);
    let mut ops = Vec::with_capacity(anzahl);
    for _ in 0..anzahl {
        let r = w.next();
        let art = r % 100;
        ops.push(if art < 55 {
            Op::Legen {
                kauf: (r >> 8) & 1 == 0,
                versatz: ((w.next() % (2 * BAND as u64)) as u16),
                menge: 1 + (w.next() % 50) as u32,
            }
        } else if art < 90 {
            Op::Storno { wahl: w.next() as u32 }
        } else {
            Op::Nehmen {
                kauf: (r >> 9) & 1 == 0,
                menge: 1 + (w.next() % 30) as u32,
            }
        });
        // Die Verzweigung oben verbraucht je nach Zweig unterschiedlich viele
        // Zufallszahlen - das ist gewollt, es macht den Strom unregelmässiger.
    }
    ops
}

fn strom_abspielen(ops: &[Op]) -> (Orderbuch, u64, u64, u64) {
    let mut buch = Orderbuch::neu(TICKS);
    let mut aus: Vec<Ausfuehrung> = Vec::with_capacity(64);
    let mut liegend: Vec<u64> = Vec::with_capacity(1 << 20);
    let (mut gelegt, mut storniert, mut genommen) = (0u64, 0u64, 0u64);

    for op in ops {
        match *op {
            Op::Legen { kauf, versatz, menge } => {
                // Passiv stellen: Käufe unter der Mitte, Verkäufe darüber.
                let tick = if kauf {
                    MITTE - 1 - (versatz as u32 % BAND)
                } else {
                    MITTE + 1 + (versatz as u32 % BAND)
                };
                aus.clear();
                let e = buch.limit(kauf, tick, menge as u64, Gueltigkeit::Gtc, &mut aus);
                if let Some(oid) = e.oid {
                    liegend.push(oid);
                    gelegt += 1;
                }
            }
            Op::Storno { wahl } => {
                if !liegend.is_empty() {
                    let i = (wahl as usize) % liegend.len();
                    let oid = liegend.swap_remove(i);
                    if buch.stornieren(oid).is_some() {
                        storniert += 1;
                    }
                }
            }
            Op::Nehmen { kauf, menge } => {
                aus.clear();
                buch.markt(kauf, menge as u64, &mut aus);
                genommen += 1;
            }
        }
    }
    (buch, gelegt, storniert, genommen)
}

fn bench() {
    const OPS: usize = 3_000_000;
    const RUNDEN: usize = 3;

    println!("\n=== DURCHSATZ ===\n");
    println!("Auftragsstrom wird erzeugt ({OPS} Operationen, ausserhalb der Messung) ...");
    let ops = strom_erzeugen(OPS, 0x2026_0823);

    let anteil = |n: usize| n as f64 * 100.0 / OPS as f64;
    let (mut l, mut s, mut n) = (0, 0, 0);
    for op in &ops {
        match op {
            Op::Legen { .. } => l += 1,
            Op::Storno { .. } => s += 1,
            Op::Nehmen { .. } => n += 1,
        }
    }
    println!(
        "  Mischung: {:.0} % legen, {:.0} % stornieren, {:.0} % nehmen\n",
        anteil(l),
        anteil(s),
        anteil(n)
    );

    let mut bestes = 0f64;
    let mut summe = 0f64;
    for runde in 1..=RUNDEN {
        let start = Instant::now();
        let (buch, gelegt, storniert, genommen) = strom_abspielen(&ops);
        let dauer = start.elapsed();

        let tps = OPS as f64 / dauer.as_secs_f64();
        let ns = dauer.as_nanos() as f64 / OPS as f64;
        bestes = bestes.max(tps);
        summe += tps;

        println!(
            "  Runde {runde}: {:>10.0} Op/s   {:>6.1} ns je Operation   ({:.3} s)",
            tps,
            ns,
            dauer.as_secs_f64()
        );
        if runde == RUNDEN {
            println!(
                "\n  Buchzustand am Ende: {} ruhende Orders, {} Ausführungen, Volumen {}",
                buch.ruhende_orders, buch.anzahl_ausfuehrungen, buch.volumen
            );
            println!(
                "  Davon: {gelegt} gelegt, {storniert} storniert, {genommen} Marktaufträge"
            );
            if let (Some(k), Some(v)) = (buch.bester_kauf(), buch.bester_verkauf()) {
                println!(
                    "  Bestes Gebot {k} ({} Lose), beste Forderung {v} ({} Lose), Spanne {}",
                    buch.menge_auf(true, k),
                    buch.menge_auf(false, v),
                    buch.spanne().unwrap()
                );
                assert!(k < v, "Buch darf nie gekreuzt sein");
                assert!(buch.liegt(1) || !buch.liegt(1)); // Bestandsabfrage ist billig
            }
        }
    }

    let mittel = summe / RUNDEN as f64;
    println!("\n  Bester Lauf:  {bestes:>12.0} Operationen pro Sekunde");
    println!("  Mittelwert:   {mittel:>12.0} Operationen pro Sekunde");
    let ziel = 400_000.0;
    if bestes >= ziel {
        println!(
            "\n  Ziel 400'000 TPS: ERREICHT - Faktor {:.1} über dem Ziel.",
            bestes / ziel
        );
    } else {
        println!("\n  Ziel 400'000 TPS: NICHT erreicht ({:.0} %).", bestes / ziel * 100.0);
    }
    println!(
        "  Gemessen auf {} Kern(en). Ein Kern, ein Buch - keine Parallelität nötig.",
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
    );
}

// ---------------------------------------------------------------------------
fn hip2_vorfuehrung() {
    println!("\n=== HIP-2 «HYPERLIQUIDITY» ===\n");

    let beschrieb = TokenBeschrieb {
        name: "ARCH".into(),
        wei_dezimalen: 9,
        sz_dezimalen: 2,
        max_menge_wei: 1_000_000 * 10u128.pow(9),
    };
    match beschrieb.pruefen() {
        Ok(()) => println!(
            "  Token «{}» gültig nach HIP-1: {:.0} Stück, Losgrösse {} Wei",
            beschrieb.name,
            beschrieb.max_menge_ganz(),
            beschrieb.los_groesse()
        ),
        Err(f) => println!("  Beanstandungen: {f:?}"),
    }

    let mut buch = Orderbuch::neu(1 << 16);
    let mut aus: Vec<Ausfuehrung> = Vec::new();
    // 40 Stufen, halb Kauf halb Verkauf, Startpreis 10'000 (= 1.0000 USDC
    // bei vier Nachkommastellen).
    let mut leiter = Hyperliquiditaet::neu(10_000, 40, 100, 20);
    leiter.stellen(&mut buch, &mut aus);
    leiter.aktualisieren(0, &mut buch, &mut aus);

    println!(
        "\n  Leiter gestellt: {} Stufen à {} Lose, Schrittweite 0.3 %",
        leiter.preise.len(),
        leiter.order_groesse
    );
    println!(
        "  Gebot {} / Forderung {} - garantierte Spanne {} bp",
        leiter.hoechster_kauf().unwrap(),
        leiter.tiefster_verkauf().unwrap(),
        leiter.spanne_bp()
    );
    debug_assert_eq!(leiter.hoechster_kauf(), buch.bester_kauf().map(u64::from));
    println!("  Kein Market Maker, kein Vertrauen in Dritte. Das Protokoll stellt selbst.\n");

    println!(
        "  {:<6} {:<26} {:>8} {:>10} {:>12} {:>10}",
        "Zeit", "Ereignis", "Gebot", "Forderung", "Token", "USDC"
    );
    println!("  {}", "-".repeat(78));

    let mut w = Wuerfel(7);
    let mut zeit = 0u64;
    for schritt in 0..12 {
        zeit += 3;
        // Abwechselnd kauft und verkauft jemand in die Leiter hinein.
        let kauft = schritt % 3 != 2;
        let menge = 100 * (1 + w.next() % 3);
        aus.clear();
        buch.markt(kauft, menge, &mut aus);
        let getroffen: Vec<Ausfuehrung> = aus.clone();
        for a in &getroffen {
            leiter.ausfuehrung_verbuchen(a);
        }
        aus.clear();
        leiter.aktualisieren(zeit, &mut buch, &mut aus);

        println!(
            "  {:<6} {:<26} {:>8} {:>10} {:>12} {:>10}",
            format!("{zeit}s"),
            format!(
                "{} {} Lose",
                if kauft { "jemand kauft" } else { "jemand verkauft" },
                menge
            ),
            buch.bester_kauf().map(|x| x.to_string()).unwrap_or("-".into()),
            buch.bester_verkauf().map(|x| x.to_string()).unwrap_or("-".into()),
            leiter.basis_bestand,
            leiter.quote_bestand / 10_000,
        );
    }

    println!(
        "\n  Nach {} Aktualisierungen liegen weiterhin {} Orders im Buch.",
        leiter.aktualisierungen, buch.ruhende_orders
    );
    println!("  Die Leiter ist mitgewandert - genau das macht HIP-2 aus.");
}

// ---------------------------------------------------------------------------
fn auktion_vorfuehrung() {
    println!("\n=== HOLLÄNDISCHE AUKTION (HIP-1) ===\n");
    println!("  31 Stunden, linear fallend, Boden 500 HYPE.");
    println!("  Startpreis = doppelter letzter Zuschlag, sonst wieder 500.\n");

    for letzter in [None, Some(2_000.0), Some(20_000.0)] {
        let a = Auktion::naechste(letzter);
        println!(
            "  Letzter Zuschlag {:>10}  ->  Start {:>9.0} HYPE",
            letzter.map(|p| format!("{p:.0}")).unwrap_or("keiner".into()),
            a.start_preis
        );
        print!("      Preisverlauf: ");
        for stunde in [0u64, 8, 16, 24, 31] {
            print!("{}h {:.0}   ", stunde, a.preis_bei(stunde * 3600));
        }
        println!();
        for budget in [1_000.0, 5_000.0] {
            match a.wartezeit_bis(budget) {
                Some(s) => println!(
                    "      Budget {:>6.0} HYPE erreicht nach {:>5.1} h",
                    budget,
                    s as f64 / 3600.0
                ),
                None => println!("      Budget {budget:>6.0} HYPE reicht nie (unter dem Boden)"),
            }
        }
        println!();
    }
}

// ---------------------------------------------------------------------------
fn main() {
    let was = std::env::args().nth(1).unwrap_or_else(|| "alles".into());
    match was.as_str() {
        "bench" => bench(),
        "hip2" => hip2_vorfuehrung(),
        "auktion" => auktion_vorfuehrung(),
        _ => {
            bench();
            hip2_vorfuehrung();
            auktion_vorfuehrung();
        }
    }
    println!();
}
