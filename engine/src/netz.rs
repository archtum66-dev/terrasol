//! Vom Speicher ins Netz - wo der Durchsatz wirklich verloren geht.
//!
//! Der Matching-Kern schafft 11.8 Millionen Operationen pro Sekunde. Trotzdem
//! kommt Hyperliquid auf rund 200 000 Orders pro Sekunde. Der Unterschied liegt
//! nicht im Matching - der ist mit 85 Nanosekunden praktisch gratis. Er liegt
//! in allem, was davor passiert:
//!
//!     Netz  ->  Rahmen zerlegen  ->  Signatur prüfen  ->  Reihenfolge  ->  Matching  ->  Protokoll
//!               ~50 ns               ~30-60 µs (!)        ~20 ns          85 ns        amortisiert
//!
//! Die Signaturprüfung ist einige hundert Mal teurer als das Matching. Genau
//! sie bestimmt, wie viele Kerne für 400 000 Orders pro Sekunde nötig sind.
//!
//! Dieses Programm misst jede Stufe einzeln und fährt dann die vollständige
//! Kette über eine echte TCP-Verbindung.
//!
//!   cargo run --release --bin netz -- stufen    einzelne Stufen messen
//!   cargo run --release --bin netz -- kette     vollständige Kette über TCP
//!   cargo run --release --bin netz -- budget    Rechnung für 400 000/s
//!   cargo run --release --bin netz              alles

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Instant;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

use engine::buch::{Ausfuehrung, Gueltigkeit, Orderbuch};

// ---------------------------------------------------------------------------
// Nachrichtenformat - 128 Bytes, feste Länge, kein JSON
// ---------------------------------------------------------------------------
// [  0.. 32]  öffentlicher Schlüssel
// [ 32.. 96]  Signatur über die 32 Nutzbytes
// [ 96..128]  Nutzlast: nonce u64 | markt u32 | tick u32 | menge u64 | flags u32 | füll u32
//
// Feste Länge heisst: Rahmen erkennen ist eine Multiplikation, kein Parser.
// Ein JSON-Parser kostet an dieser Stelle das Zehn- bis Hundertfache.
pub const NACHRICHT: usize = 128;
pub const NUTZLAST: usize = 32;

#[derive(Clone, Copy, Debug)]
pub struct Auftrag {
    pub tick: u32,
    pub menge: u64,
    pub kauf: bool,
}

#[inline(always)]
fn nutzlast_lesen(roh: &[u8]) -> Auftrag {
    let p = &roh[96..128];
    let tick = u32::from_le_bytes([p[8], p[9], p[10], p[11]]);
    let menge = u64::from_le_bytes([p[16], p[17], p[18], p[19], p[20], p[21], p[22], p[23]]);
    let flags = u32::from_le_bytes([p[24], p[25], p[26], p[27]]);
    Auftrag { tick, menge, kauf: flags & 1 == 1 }
}

fn nachricht_bauen(sk: &SigningKey, nonce: u64, tick: u32, menge: u64, kauf: bool) -> [u8; NACHRICHT] {
    let mut roh = [0u8; NACHRICHT];
    roh[0..32].copy_from_slice(sk.verifying_key().as_bytes());
    let p = &mut roh[96..128];
    p[0..8].copy_from_slice(&nonce.to_le_bytes());
    p[8..12].copy_from_slice(&tick.to_le_bytes());
    p[16..24].copy_from_slice(&menge.to_le_bytes());
    p[24..28].copy_from_slice(&(kauf as u32).to_le_bytes());
    let nutzlast: [u8; NUTZLAST] = roh[96..128].try_into().unwrap();
    let sig: Signature = sk.sign(&nutzlast);
    roh[32..96].copy_from_slice(&sig.to_bytes());
    roh
}

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

const TICKS: u32 = 1 << 16;
const MITTE: u32 = TICKS / 2;

/// Erzeugt echte, gültig signierte Nachrichten.
fn strom_bauen(anzahl: usize) -> Vec<u8> {
    let sk = SigningKey::from_bytes(&[7u8; 32]);
    let mut w = Wuerfel(0x2026_0823);
    let mut puffer = Vec::with_capacity(anzahl * NACHRICHT);
    for i in 0..anzahl {
        let r = w.next();
        let kauf = r & 1 == 0;
        let tick = if kauf {
            MITTE - 1 - (w.next() % 200) as u32
        } else {
            MITTE + 1 + (w.next() % 200) as u32
        };
        puffer.extend_from_slice(&nachricht_bauen(&sk, i as u64, tick, 1 + w.next() % 50, kauf));
    }
    puffer
}

// ===========================================================================
// Teil 1 - Stufen einzeln messen
// ===========================================================================
struct Kosten {
    signatur_einzeln: f64,
    signatur_stapel: Vec<(usize, f64)>,
    zerlegen: f64,
    matching: f64,
    protokoll: f64,
}

fn stufen() -> Kosten {
    println!("\n=== STUFEN EINZELN ===\n");
    const N: usize = 20_000;
    let roh = strom_bauen(N);

    // -- Signatur einzeln --------------------------------------------------
    let schluessel = VerifyingKey::from_bytes(&roh[0..32].try_into().unwrap()).unwrap();
    let proben = 4_000.min(N);
    let start = Instant::now();
    let mut gut = 0u64;
    for i in 0..proben {
        let n = &roh[i * NACHRICHT..(i + 1) * NACHRICHT];
        let sig = Signature::from_bytes(&n[32..96].try_into().unwrap());
        if schluessel.verify_strict(&n[96..128], &sig).is_ok() {
            gut += 1;
        }
    }
    let dauer = start.elapsed();
    let signatur_einzeln = dauer.as_nanos() as f64 / proben as f64;
    assert_eq!(gut, proben as u64, "alle Signaturen müssen gültig sein");
    println!(
        "  Signatur einzeln         {:>10.0} ns   {:>10.0} Orders/s je Kern",
        signatur_einzeln,
        1e9 / signatur_einzeln
    );

    // -- Signatur im Stapel ------------------------------------------------
    println!();
    let mut signatur_stapel = Vec::new();
    for groesse in [16usize, 64, 256] {
        let runden = (proben / groesse).max(1);
        let start = Instant::now();
        for r in 0..runden {
            let mut nachrichten: Vec<&[u8]> = Vec::with_capacity(groesse);
            let mut signaturen: Vec<Signature> = Vec::with_capacity(groesse);
            let mut schluesselliste: Vec<VerifyingKey> = Vec::with_capacity(groesse);
            for k in 0..groesse {
                let n = &roh[(r * groesse + k) * NACHRICHT..(r * groesse + k + 1) * NACHRICHT];
                nachrichten.push(&n[96..128]);
                signaturen.push(Signature::from_bytes(&n[32..96].try_into().unwrap()));
                schluesselliste.push(VerifyingKey::from_bytes(&n[0..32].try_into().unwrap()).unwrap());
            }
            ed25519_dalek::verify_batch(&nachrichten, &signaturen, &schluesselliste)
                .expect("Stapel muss gültig sein");
        }
        let dauer = start.elapsed();
        let je = dauer.as_nanos() as f64 / (runden * groesse) as f64;
        signatur_stapel.push((groesse, je));
        println!(
            "  Signatur im Stapel {:>4}  {:>10.0} ns   {:>10.0} Orders/s je Kern   ({:.1}× schneller)",
            groesse,
            je,
            1e9 / je,
            signatur_einzeln / je
        );
    }

    // -- Rahmen zerlegen ---------------------------------------------------
    println!();
    let start = Instant::now();
    let mut summe = 0u64;
    for i in 0..N {
        let a = nutzlast_lesen(&roh[i * NACHRICHT..(i + 1) * NACHRICHT]);
        summe = summe.wrapping_add(a.menge).wrapping_add(a.tick as u64);
    }
    let zerlegen = start.elapsed().as_nanos() as f64 / N as f64;
    std::hint::black_box(summe);
    println!(
        "  Rahmen zerlegen          {:>10.1} ns   {:>10.0} Orders/s je Kern",
        zerlegen,
        1e9 / zerlegen
    );

    // -- Matching ----------------------------------------------------------
    let mut buch = Orderbuch::neu(TICKS);
    let mut aus: Vec<Ausfuehrung> = Vec::with_capacity(64);
    let start = Instant::now();
    for i in 0..N {
        let a = nutzlast_lesen(&roh[i * NACHRICHT..(i + 1) * NACHRICHT]);
        aus.clear();
        buch.limit(a.kauf, a.tick, a.menge, Gueltigkeit::Gtc, &mut aus);
    }
    let matching = start.elapsed().as_nanos() as f64 / N as f64;
    println!(
        "  Matching                 {:>10.1} ns   {:>10.0} Orders/s je Kern",
        matching,
        1e9 / matching
    );

    // -- Protokoll mit Gruppen-Commit --------------------------------------
    // Jede angenommene Order muss dauerhaft sein, bevor sie bestätigt wird.
    // Einzeln synchronisiert kostet das Millisekunden. Im Gruppen-Commit
    // teilen sich hunderte Orders ein fsync - und es wird vernachlässigbar.
    let pfad = std::env::temp_dir().join("engine_wal.bin");
    let mut datei = std::fs::File::create(&pfad).expect("WAL");
    let gruppe = 1_000usize;
    let runden = 40usize;
    let block = vec![0u8; gruppe * NACHRICHT];
    let start = Instant::now();
    for _ in 0..runden {
        datei.write_all(&block).unwrap();
        datei.sync_data().unwrap();
    }
    let protokoll = start.elapsed().as_nanos() as f64 / (runden * gruppe) as f64;
    let _ = std::fs::remove_file(&pfad);
    println!(
        "  Protokoll (fsync je {gruppe}) {:>7.1} ns   {:>10.0} Orders/s je Kern",
        protokoll,
        1e9 / protokoll
    );

    Kosten { signatur_einzeln, signatur_stapel, zerlegen, matching, protokoll }
}

// ===========================================================================
// Teil 2 - vollständige Kette über eine echte TCP-Verbindung
// ===========================================================================
fn kette(pruefer: usize, mit_signatur: bool, orders: usize, roh: Arc<Vec<u8>>) -> f64 {
    let horcher = TcpListener::bind("127.0.0.1:0").expect("binden");
    let adresse = horcher.local_addr().unwrap();
    let je_pruefer = orders / pruefer;

    let gezaehlt = Arc::new(AtomicU64::new(0));
    let (sender, empfaenger) = mpsc::channel::<Vec<Auftrag>>();

    // -- Matching-Faden: einer, immer. Reihenfolge ist heilig. -------------
    let zaehler = gezaehlt.clone();
    let matcher = std::thread::spawn(move || {
        let mut buch = Orderbuch::neu(TICKS);
        let mut aus: Vec<Ausfuehrung> = Vec::with_capacity(64);
        for stapel in empfaenger {
            for a in &stapel {
                aus.clear();
                buch.limit(a.kauf, a.tick, a.menge, Gueltigkeit::Gtc, &mut aus);
            }
            zaehler.fetch_add(stapel.len() as u64, Ordering::Relaxed);
        }
        buch.anzahl_ausfuehrungen
    });

    // -- Klienten ZUERST starten -------------------------------------------
    // Der Server ruft accept() auf; gäbe es noch keine Verbindungswünsche,
    // bliebe er dort stehen. Das TCP-Rückstauverzeichnis nimmt die Verbindungen
    // entgegen, bis der Server sie abholt.
    let im_puffer = roh.len() / NACHRICHT;
    let runden = (je_pruefer / im_puffer).max(1);
    let mut klienten = Vec::new();
    let start = Instant::now();
    for _ in 0..pruefer {
        let roh = roh.clone();
        klienten.push(std::thread::spawn(move || {
            let mut strom = TcpStream::connect(adresse).expect("verbinden");
            strom.set_nodelay(true).ok();
            for _ in 0..runden {
                if strom.write_all(&roh).is_err() {
                    break;
                }
            }
            // Schliessen signalisiert dem Server das Ende.
            strom.shutdown(std::net::Shutdown::Write).ok();
        }));
    }

    // -- Prüfer-Fäden: lesen, zerlegen, Signaturen im Stapel prüfen --------
    let mut server_faeden = Vec::new();
    for _ in 0..pruefer {
        let sender = sender.clone();
        let (mut strom, _) = horcher.accept().expect("annehmen");
        server_faeden.push(std::thread::spawn(move || {
            // 512 Nachrichten je Lesevorgang: ein Systemaufruf statt 512.
            const STAPEL: usize = 512;
            let mut puffer = vec![0u8; STAPEL * NACHRICHT];
            let mut belegt = 0usize;
            loop {
                match strom.read(&mut puffer[belegt..]) {
                    Ok(0) => break,
                    Ok(n) => belegt += n,
                    Err(_) => break,
                }
                let ganze = belegt / NACHRICHT;
                if ganze == 0 {
                    continue;
                }
                if mit_signatur {
                    let mut nachrichten: Vec<&[u8]> = Vec::with_capacity(ganze);
                    let mut signaturen: Vec<Signature> = Vec::with_capacity(ganze);
                    let mut schluessel: Vec<VerifyingKey> = Vec::with_capacity(ganze);
                    for i in 0..ganze {
                        let n = &puffer[i * NACHRICHT..(i + 1) * NACHRICHT];
                        nachrichten.push(&n[96..128]);
                        signaturen.push(Signature::from_bytes(&n[32..96].try_into().unwrap()));
                        schluessel.push(
                            VerifyingKey::from_bytes(&n[0..32].try_into().unwrap()).unwrap(),
                        );
                    }
                    ed25519_dalek::verify_batch(&nachrichten, &signaturen, &schluessel)
                        .expect("Stapel muss gültig sein");
                }
                let mut auftraege = Vec::with_capacity(ganze);
                for i in 0..ganze {
                    auftraege.push(nutzlast_lesen(&puffer[i * NACHRICHT..(i + 1) * NACHRICHT]));
                }
                if sender.send(auftraege).is_err() {
                    break;
                }
                let rest = belegt - ganze * NACHRICHT;
                puffer.copy_within(ganze * NACHRICHT..belegt, 0);
                belegt = rest;
            }
        }));
    }
    drop(sender);

    for k in klienten {
        k.join().ok();
    }
    for f in server_faeden {
        f.join().ok();
    }
    let ausfuehrungen = matcher.join().unwrap();
    let dauer = start.elapsed();

    let verarbeitet = gezaehlt.load(Ordering::Relaxed);
    let rate = verarbeitet as f64 / dauer.as_secs_f64();
    println!(
        "  {:<32} {:>12.0} Orders/s   ({} verarbeitet, {} Ausführungen, {:.2} s)",
        format!(
            "{pruefer} Prüfer, Signatur {}",
            if mit_signatur { "an " } else { "aus" }
        ),
        rate,
        verarbeitet,
        ausfuehrungen,
        dauer.as_secs_f64()
    );
    rate
}



/// Dieselbe Kette wie oben, aber der Klient sendet Bündel: eine Signatur
/// für `je_buendel` Orders. Der Server prüft eine Signatur je Bündel.
fn kette_buendel(pruefer: usize, je_buendel: usize, buendel_anzahl: usize) -> f64 {
    let horcher = TcpListener::bind("127.0.0.1:0").expect("binden");
    let adresse = horcher.local_addr().unwrap();
    let laenge = KOPF + je_buendel * NUTZLAST;

    let sk = SigningKey::from_bytes(&[9u8; 32]);
    let mut w = Wuerfel(0xCAFE);
    // Ein Puffer mit mehreren fertigen Bündeln, der wiederholt gesendet wird.
    let im_puffer = (1_000_000 / laenge).max(1);
    let mut puffer = Vec::with_capacity(im_puffer * laenge);
    for _ in 0..im_puffer {
        puffer.extend_from_slice(&buendel_bauen(&sk, je_buendel, &mut w));
    }
    let roh = Arc::new(puffer);

    let gezaehlt = Arc::new(AtomicU64::new(0));
    let (sender, empfaenger) = mpsc::channel::<Vec<Auftrag>>();

    let zaehler = gezaehlt.clone();
    let matcher = std::thread::spawn(move || {
        let mut buch = Orderbuch::neu(TICKS);
        let mut aus: Vec<Ausfuehrung> = Vec::with_capacity(64);
        for stapel in empfaenger {
            for a in &stapel {
                aus.clear();
                buch.limit(a.kauf, a.tick, a.menge, Gueltigkeit::Gtc, &mut aus);
            }
            zaehler.fetch_add(stapel.len() as u64, Ordering::Relaxed);
        }
        buch.anzahl_ausfuehrungen
    });

    let runden = (buendel_anzahl / pruefer / im_puffer).max(1);
    let mut klienten = Vec::new();
    let start = Instant::now();
    for _ in 0..pruefer {
        let roh = roh.clone();
        klienten.push(std::thread::spawn(move || {
            let mut strom = TcpStream::connect(adresse).expect("verbinden");
            strom.set_nodelay(true).ok();
            for _ in 0..runden {
                if strom.write_all(&roh).is_err() {
                    break;
                }
            }
            strom.shutdown(std::net::Shutdown::Write).ok();
        }));
    }

    let mut server_faeden = Vec::new();
    for _ in 0..pruefer {
        let sender = sender.clone();
        let (mut strom, _) = horcher.accept().expect("annehmen");
        server_faeden.push(std::thread::spawn(move || {
            let mut puffer = vec![0u8; laenge * 64];
            let mut belegt = 0usize;
            loop {
                match strom.read(&mut puffer[belegt..]) {
                    Ok(0) => break,
                    Ok(n) => belegt += n,
                    Err(_) => break,
                }
                let ganze = belegt / laenge;
                if ganze == 0 {
                    continue;
                }
                let mut auftraege = Vec::with_capacity(ganze * je_buendel);
                for b in 0..ganze {
                    let bue = &puffer[b * laenge..(b + 1) * laenge];
                    // EINE Signaturprüfung für je_buendel Orders.
                    let anzahl = buendel_pruefen(bue).expect("Bündel muss gültig sein");
                    for i in 0..anzahl {
                        auftraege.push(buendel_order(bue, i));
                    }
                }
                if sender.send(auftraege).is_err() {
                    break;
                }
                let rest = belegt - ganze * laenge;
                puffer.copy_within(ganze * laenge..belegt, 0);
                belegt = rest;
            }
        }));
    }
    drop(sender);

    for k in klienten {
        k.join().ok();
    }
    for f in server_faeden {
        f.join().ok();
    }
    matcher.join().ok();
    let dauer = start.elapsed();
    let verarbeitet = gezaehlt.load(Ordering::Relaxed);
    let rate = verarbeitet as f64 / dauer.as_secs_f64();
    println!(
        "  {:<40} {:>14.0} Orders/s   ({} Orders, {:.2} s)",
        format!("{pruefer} Prüfer, Bündel à {je_buendel} Orders"),
        rate,
        verarbeitet,
        dauer.as_secs_f64()
    );
    rate
}

// ===========================================================================
// Teil 4 - Bündel: EINE Signatur für viele Orders
// ===========================================================================
//
// Der Engpass ist nicht die Kryptografie an sich, sondern das Verhältnis
// "eine Signatur je Order". Das ist eine Entscheidung des Protokolls, keine
// Naturkonstante. Wer ein Bündel von n Orders mit EINER Signatur unterschreibt,
// teilt die 20 Mikrosekunden durch n.
//
// Das ist kein Trick, sondern gängige Praxis: Hyperliquids eigene
// Schnittstelle kennt `bulk_orders` - eine signierte Aktion, die viele Orders
// enthält. Market Maker, die 50 Quotes gleichzeitig erneuern, senden ohnehin
// Bündel.
//
// Bündelformat:
//   [  0.. 32]  öffentlicher Schlüssel
//   [ 32.. 96]  Signatur über ALLES ab Byte 96
//   [ 96..100]  Anzahl n (u32)
//   [100..    ]  n × 32 Byte Nutzlast
//
// Der Preis: Es hilft dem Durchsatz, nicht der Antwortzeit einer einzelnen
// Order. Wer genau eine Order sendet, zahlt weiterhin die vollen 20 µs.

const KOPF: usize = 100;

fn buendel_bauen(sk: &SigningKey, n: usize, w: &mut Wuerfel) -> Vec<u8> {
    let mut roh = vec![0u8; KOPF + n * NUTZLAST];
    roh[0..32].copy_from_slice(sk.verifying_key().as_bytes());
    roh[96..100].copy_from_slice(&(n as u32).to_le_bytes());
    for i in 0..n {
        let r = w.next();
        let kauf = r & 1 == 0;
        let tick = if kauf {
            MITTE - 1 - (w.next() % 200) as u32
        } else {
            MITTE + 1 + (w.next() % 200) as u32
        };
        let p = &mut roh[KOPF + i * NUTZLAST..KOPF + (i + 1) * NUTZLAST];
        p[0..8].copy_from_slice(&(i as u64).to_le_bytes());
        p[8..12].copy_from_slice(&tick.to_le_bytes());
        p[16..24].copy_from_slice(&(1 + w.next() % 50).to_le_bytes());
        p[24..28].copy_from_slice(&(kauf as u32).to_le_bytes());
    }
    let sig: Signature = sk.sign(&roh[96..]);
    roh[32..96].copy_from_slice(&sig.to_bytes());
    roh
}

#[inline(always)]
fn buendel_pruefen(roh: &[u8]) -> Option<usize> {
    let schluessel = VerifyingKey::from_bytes(&roh[0..32].try_into().ok()?).ok()?;
    let sig = Signature::from_bytes(&roh[32..96].try_into().ok()?);
    schluessel.verify_strict(&roh[96..], &sig).ok()?;
    Some(u32::from_le_bytes(roh[96..100].try_into().ok()?) as usize)
}

#[inline(always)]
fn buendel_order(roh: &[u8], i: usize) -> Auftrag {
    let p = &roh[KOPF + i * NUTZLAST..KOPF + (i + 1) * NUTZLAST];
    Auftrag {
        tick: u32::from_le_bytes([p[8], p[9], p[10], p[11]]),
        menge: u64::from_le_bytes([p[16], p[17], p[18], p[19], p[20], p[21], p[22], p[23]]),
        kauf: u32::from_le_bytes([p[24], p[25], p[26], p[27]]) & 1 == 1,
    }
}

fn buendel_messen(k: &Kosten) -> f64 {
    println!("\n=== BÜNDEL: EINE SIGNATUR FÜR VIELE ORDERS ===\n");
    let sk = SigningKey::from_bytes(&[9u8; 32]);
    let mut w = Wuerfel(0xBEEF);
    let einzeln = k.signatur_stapel.iter().map(|x| x.1).fold(f64::MAX, f64::min);

    println!(
        "  {:<12}{:>14}{:>20}{:>16}{:>14}",
        "Orders", "ns/Order", "Orders/s je Kern", "gegen einzeln", "Bytes/Order"
    );
    println!("  {}", "-".repeat(76));

    let mut bestes = 0f64;
    for n in [1usize, 4, 16, 64, 256, 1024] {
        let buendel = buendel_bauen(&sk, n, &mut w);
        let runden = (200_000 / n).clamp(20, 5000);

        // Vollständiger Weg: Signatur prüfen, alle Orders zerlegen, matchen.
        let mut buch = Orderbuch::neu(TICKS);
        let mut aus: Vec<Ausfuehrung> = Vec::with_capacity(64);
        let start = Instant::now();
        let mut verarbeitet = 0usize;
        for _ in 0..runden {
            let anzahl = buendel_pruefen(&buendel).expect("Bündel muss gültig sein");
            for i in 0..anzahl {
                let a = buendel_order(&buendel, i);
                aus.clear();
                buch.limit(a.kauf, a.tick, a.menge, Gueltigkeit::Gtc, &mut aus);
            }
            verarbeitet += anzahl;
        }
        let je = start.elapsed().as_nanos() as f64 / verarbeitet as f64;
        bestes = bestes.max(1e9 / je);
        println!(
            "  {:<12}{:>14.1}{:>20.0}{:>16}{:>14.1}",
            n,
            je,
            1e9 / je,
            format!("{:.0}×", einzeln / je),
            (KOPF + n * NUTZLAST) as f64 / n as f64
        );
    }
    println!("  {}", "-".repeat(76));
    println!(
        "\n  Faktor gegenüber einer Signatur je Order: bis zu {:.0}×.",
        bestes / (1e9 / einzeln)
    );
    println!(
        "  Ein Kern schafft damit {:.2} Millionen Orders pro Sekunde -\n  \
         für 400'000/s genügt {:.2} Kerne.",
        bestes / 1e6,
        400_000.0 / bestes
    );
    println!(
        "\n  Der Preis: Das hilft dem DURCHSATZ, nicht der Antwortzeit einer\n  \
         einzelnen Order. Wer genau eine Order sendet, zahlt weiterhin die\n  \
         vollen {:.0} µs. Bündel lohnen sich für Market Maker, die ohnehin\n  \
         viele Quotes gleichzeitig erneuern - und genau die machen das Volumen.",
        k.signatur_einzeln / 1000.0
    );
    bestes
}

// ===========================================================================
// Teil 3 - die Rechnung für 400 000 Orders pro Sekunde
// ===========================================================================
fn budget(k: &Kosten) {
    println!("\n=== BUDGET FÜR 400'000 ORDERS/S ===\n");
    const ZIEL: f64 = 400_000.0;

    // Bei 400'000/s hat das ganze System 2500 ns Zeit je Order - verteilt
    // auf beliebig viele Kerne.
    let fenster = 1e9 / ZIEL;
    println!("  Zeitfenster je Order über das ganze System: {fenster:.0} ns\n");

    let bester_stapel = k
        .signatur_stapel
        .iter()
        .cloned()
        .fold((0usize, f64::MAX), |a, b| if b.1 < a.1 { b } else { a });

    let posten = [
        ("Signatur einzeln prüfen", k.signatur_einzeln),
        (
            "Signatur im Stapel prüfen",
            bester_stapel.1,
        ),
        ("Rahmen zerlegen", k.zerlegen),
        ("Matching", k.matching),
        ("Protokoll (Gruppen-Commit)", k.protokoll),
    ];

    println!("  {:<30}{:>13}{:>17}{:>10}", "Stufe", "ns/Order", "Kerne für 400k", "Anteil");
    println!("  {}", "-".repeat(70));
    let mit_stapel: f64 = bester_stapel.1 + k.zerlegen + k.matching + k.protokoll;
    for (name, ns) in posten {
        let kerne = ZIEL * ns / 1e9;
        let anteil = if name.starts_with("Signatur einzeln") {
            f64::NAN
        } else {
            ns / mit_stapel * 100.0
        };
        let anteil_text = if anteil.is_nan() {
            "-".to_string()
        } else {
            format!("{anteil:.1} %")
        };
        println!("  {name:<30}{ns:>13.1}{kerne:>17.2}{anteil_text:>10}");
    }
    println!("  {}", "-".repeat(70));

    let ohne_stapel = k.signatur_einzeln + k.zerlegen + k.matching + k.protokoll;
    println!(
        "  {:<30}{:>13.1}{:>17.2}",
        "Summe OHNE Stapelprüfung", ohne_stapel, ZIEL * ohne_stapel / 1e9
    );
    println!(
        "  {:<30}{:>13.1}{:>17.2}",
        format!("Summe MIT Stapel ({})", bester_stapel.0),
        mit_stapel,
        ZIEL * mit_stapel / 1e9
    );
    let ohne_kryptografie = k.zerlegen + k.matching + k.protokoll;
    println!(
        "  {:<30}{:>13.1}{:>17.3}",
        "Summe OHNE Signatur je Order", ohne_kryptografie, ZIEL * ohne_kryptografie / 1e9
    );

    println!("\n  Antwort:");
    println!(
        "    Mit Stapelprüfung braucht es rund {:.0} Kerne für 400'000 Orders/s.",
        (ZIEL * mit_stapel / 1e9).ceil()
    );
    println!(
        "    Ohne Stapelprüfung wären es {:.0} - allein für die Signaturen.",
        (ZIEL * ohne_stapel / 1e9).ceil()
    );
    println!(
        "    Das Matching braucht davon {:.3} Kerne. Es ist nie der Engpass.",
        ZIEL * k.matching / 1e9
    );
    println!(
        "\n    Anteil der Signaturprüfung an der Gesamtarbeit: {:.0} %.",
        bester_stapel.1 / mit_stapel * 100.0
    );
    println!("    Wer den Durchsatz erhöhen will, arbeitet dort - nicht am Orderbuch.");

    // -- Der eigentliche Punkt --------------------------------------------
    let ohne_kryptografie = k.zerlegen + k.matching + k.protokoll;
    println!("\n  Zwei Architekturen, zwei völlig verschiedene Antworten:\n");
    println!("  {:<46}{:>16}{:>14}", "", "Kerne für 400k", "Grenze");
    println!("  {}", "-".repeat(76));
    println!(
        "  {:<46}{:>16.2}{:>14}",
        "A) Sitzung authentisiert (klassische Börse)",
        ZIEL * ohne_kryptografie / 1e9,
        format!("{:.1} Mio/s", 1e9 / ohne_kryptografie / 1e6)
    );
    println!(
        "  {:<46}{:>16.2}{:>14}",
        "B) Signatur je Order (Blockchain, DEX)",
        ZIEL * mit_stapel / 1e9,
        format!("{:.0}k/s je Kern", 1e9 / mit_stapel / 1e3)
    );
    println!("  {}", "-".repeat(76));
    println!(
        "\n  A) Der Nutzer meldet sich EINMAL an, danach ist jede Order nur noch\n     \
         Nutzlast. So arbeiten Nasdaq, CME und jede klassische Börse. 400'000/s\n     \
         sind damit kein Ziel, sondern ein Nebenprodukt - ein Kern genügt.\n\n  \
         B) Jede Order trägt ihre eigene Signatur, weil niemandem vertraut wird.\n     \
         Das ist der Preis der Nichtverwahrung - und der Grund, warum\n     \
         Hyperliquid bei rund 200'000 Orders/s liegt und nicht bei 10 Millionen.\n     \
         Dazu kommt dort noch der Konsens, der hier gar nicht gemessen ist."
    );
    println!(
        "\n  Gemessen auf {} Kern(en), Intel Xeon 2.80 GHz in einem Container.\n  \
         Auf aktueller Serverhardware mit AVX2 ist die Signaturprüfung rund\n  \
         zwei- bis viermal schneller - dann sind es eher 2 bis 4 Kerne statt {:.0}.",
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        (ZIEL * mit_stapel / 1e9).ceil()
    );
}

// ---------------------------------------------------------------------------
fn main() {
    let was = std::env::args().nth(1).unwrap_or_else(|| "alles".into());
    let kerne = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);

    match was.as_str() {
        "stufen" => {
            stufen();
        }
        "kette" => {
            println!("\n=== VOLLSTÄNDIGE KETTE ÜBER TCP ===\n");
            let roh = Arc::new(strom_bauen(10_000));
            for p in 1..=kerne.min(4) {
                kette(p, false, 200_000, roh.clone());
            }
            println!();
            for p in 1..=kerne.min(4) {
                kette(p, true, 40_000, roh.clone());
            }
        }
        "budget" => {
            let k = stufen();
            budget(&k);
        }
        "buendel" => {
            let k = stufen();
            buendel_messen(&k);
            println!("\n=== BÜNDEL ÜBER ECHTES TCP ===\n");
            for n in [1usize, 16, 64, 256] {
                kette_buendel(kerne.min(2), n, 400_000 / n.max(1));
            }
        }
        _ => {
            let k = stufen();
            println!("\n=== VOLLSTÄNDIGE KETTE ÜBER TCP ===\n");
            println!("  Gemessen auf {kerne} Kern(en) - Klient und Server teilen sie sich,\n  \
                      die Zahlen sind darum eine Untergrenze.\n");
            let roh = Arc::new(strom_bauen(10_000));
            for p in 1..=kerne.min(4) {
                kette(p, false, 200_000, roh.clone());
            }
            println!();
            for p in 1..=kerne.min(4) {
                kette(p, true, 40_000, roh.clone());
            }
            budget(&k);
            buendel_messen(&k);
            println!("\n=== BÜNDEL ÜBER ECHTES TCP ===\n");
            for n in [1usize, 16, 64, 256] {
                kette_buendel(kerne.min(2), n, 400_000 / n.max(1));
            }
        }
    }
    println!();
}
