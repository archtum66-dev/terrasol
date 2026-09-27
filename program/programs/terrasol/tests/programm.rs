//! Programmtests gegen die echte Solana-Laufzeit (Bank + SPL Token), nativ
//! ausgeführt mit `solana-program-test`. Anders als der Python-Lauf gegen den
//! lokalen Validator lässt sich hier die Uhr stellen - nur so ist `unstake`
//! nach Ablauf der 7-Tage-Sperre überhaupt prüfbar.
//!
//! Aufruf:  cargo test --manifest-path program/Cargo.toml

use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use solana_program_test::{processor, BanksClientError, ProgramTest, ProgramTestContext};
use solana_sdk::{
    account::Account,
    account_info::AccountInfo,
    bpf_loader_upgradeable::{self, UpgradeableLoaderState},
    clock::Clock,
    entrypoint::ProgramResult,
    instruction::{Instruction, InstructionError},
    program_pack::Pack,
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
    system_instruction, system_program, sysvar,
    transaction::{Transaction, TransactionError},
};
use terrasol::{accounts as konten, instruction as ix, Config, Listing, StakePosition, TerraError};

const TAG: i64 = 24 * 60 * 60;
const STUFEN: [u64; 4] = [100, 1_000, 10_000, 100_000];

// Anchor erzeugt `entry` mit Lebensdauern, die `processor!` nicht kennt.
fn einstieg(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    let accounts: &[AccountInfo] = unsafe { std::mem::transmute(accounts) };
    terrasol::entry(program_id, accounts, data)
}

fn fehler(e: TerraError) -> u32 {
    u32::from(e)
}

fn fehlercode(e: BanksClientError) -> Option<u32> {
    match e {
        BanksClientError::TransactionError(TransactionError::InstructionError(
            _,
            InstructionError::Custom(c),
        ))
        | BanksClientError::SimulationError {
            err: TransactionError::InstructionError(_, InstructionError::Custom(c)),
            ..
        } => Some(c),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
struct Welt {
    ctx: ProgramTestContext,
    admin: Keypair,
    governance: Keypair,
    oracle: Keypair,
    mint: Keypair,
    config: Pubkey,
    vault: Pubkey,
}

struct Person {
    kp: Keypair,
    token: Pubkey,
}

impl Welt {
    /// Frische Kette. `admin` ist die Upgrade-Autorität des Programms.
    async fn neu() -> Self {
        let mut pt = ProgramTest::new("terrasol", terrasol::ID, processor!(einstieg));
        pt.prefer_bpf(false);

        let admin = Keypair::new();
        pt.add_account(
            admin.pubkey(),
            Account { lamports: 100_000_000_000, owner: system_program::ID, ..Account::default() },
        );
        // Das ProgramData-Konto, das ein echter `solana program deploy` anlegt.
        let (program_data, _) =
            Pubkey::find_program_address(&[terrasol::ID.as_ref()], &bpf_loader_upgradeable::ID);
        let daten = bincode::serialize(&UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: Some(admin.pubkey()),
        })
        .unwrap();
        pt.add_account(
            program_data,
            Account {
                lamports: 1_000_000_000,
                data: daten,
                owner: bpf_loader_upgradeable::ID,
                executable: false,
                rent_epoch: 0,
            },
        );

        let ctx = pt.start_with_context().await;
        let (config, _) = Pubkey::find_program_address(&[b"config"], &terrasol::ID);
        let (vault, _) = Pubkey::find_program_address(&[b"vault", config.as_ref()], &terrasol::ID);
        let mut w = Welt {
            ctx,
            admin,
            governance: Keypair::new(),
            oracle: Keypair::new(),
            mint: Keypair::new(),
            config,
            vault,
        };
        w.mint_anlegen().await;
        w
    }

    async fn senden(&mut self, ixs: &[Instruction], signer: &[&Keypair]) -> Result<(), BanksClientError> {
        let bh = self.ctx.get_new_latest_blockhash().await.unwrap();
        let tx = Transaction::new_signed_with_payer(ixs, Some(&signer[0].pubkey()), signer, bh);
        self.ctx.banks_client.process_transaction(tx).await
    }

    async fn sol(&mut self, an: &Pubkey, lamports: u64) {
        let zahler = self.ctx.payer.insecure_clone();
        self.senden(&[system_instruction::transfer(&zahler.pubkey(), an, lamports)], &[&zahler])
            .await
            .unwrap();
    }

    async fn mint_anlegen(&mut self) {
        let zahler = self.ctx.payer.insecure_clone();
        let mint = self.mint.insecure_clone();
        let miete = self.ctx.banks_client.get_rent().await.unwrap().minimum_balance(spl_token::state::Mint::LEN);
        self.senden(
            &[
                system_instruction::create_account(
                    &zahler.pubkey(),
                    &mint.pubkey(),
                    miete,
                    spl_token::state::Mint::LEN as u64,
                    &spl_token::ID,
                ),
                spl_token::instruction::initialize_mint(&spl_token::ID, &mint.pubkey(), &zahler.pubkey(), None, 0)
                    .unwrap(),
            ],
            &[&zahler, &mint],
        )
        .await
        .unwrap();
    }

    /// Neue Person mit SOL und `menge` TRRA.
    async fn person(&mut self, menge: u64) -> Person {
        let zahler = self.ctx.payer.insecure_clone();
        let kp = Keypair::new();
        let konto = Keypair::new();
        self.sol(&kp.pubkey(), 1_000_000_000).await;
        let miete =
            self.ctx.banks_client.get_rent().await.unwrap().minimum_balance(spl_token::state::Account::LEN);
        self.senden(
            &[
                system_instruction::create_account(
                    &zahler.pubkey(),
                    &konto.pubkey(),
                    miete,
                    spl_token::state::Account::LEN as u64,
                    &spl_token::ID,
                ),
                spl_token::instruction::initialize_account(&spl_token::ID, &konto.pubkey(), &self.mint.pubkey(), &kp.pubkey())
                    .unwrap(),
                spl_token::instruction::mint_to(&spl_token::ID, &self.mint.pubkey(), &konto.pubkey(), &zahler.pubkey(), &[], menge)
                    .unwrap(),
            ],
            &[&zahler, &konto],
        )
        .await
        .unwrap();
        Person { kp, token: konto.pubkey() }
    }

    fn init_ix(&self, payer: &Pubkey) -> Instruction {
        let (program_data, _) =
            Pubkey::find_program_address(&[terrasol::ID.as_ref()], &bpf_loader_upgradeable::ID);
        Instruction {
            program_id: terrasol::ID,
            accounts: konten::Initialize {
                config: self.config,
                governance: self.governance.pubkey(),
                oracle: self.oracle.pubkey(),
                stake_mint: self.mint.pubkey(),
                vault: self.vault,
                payer: *payer,
                program_data,
                token_program: spl_token::ID,
                system_program: system_program::ID,
                rent: sysvar::rent::ID,
            }
            .to_account_metas(None),
            data: ix::Initialize { tier_thresholds: STUFEN }.data(),
        }
    }

    async fn initialisieren(&mut self) {
        let admin = self.admin.insecure_clone();
        let i = self.init_ix(&admin.pubkey());
        self.senden(&[i], &[&admin]).await.unwrap();
        self.sol(&self.governance.pubkey(), 1_000_000_000).await;
        self.sol(&self.oracle.pubkey(), 1_000_000_000).await;
    }

    fn position(&self, wer: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(&[b"position", wer.as_ref()], &terrasol::ID).0
    }

    fn stake_ix(&self, p: &Person, menge: u64) -> Instruction {
        Instruction {
            program_id: terrasol::ID,
            accounts: konten::Stake {
                config: self.config,
                position: self.position(&p.kp.pubkey()),
                vault: self.vault,
                user_token: p.token,
                user: p.kp.pubkey(),
                token_program: spl_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: ix::Stake { amount: menge }.data(),
        }
    }

    fn unstake_ix(&self, p: &Person, menge: u64) -> Instruction {
        Instruction {
            program_id: terrasol::ID,
            accounts: konten::Unstake {
                config: self.config,
                position: self.position(&p.kp.pubkey()),
                vault: self.vault,
                user_token: p.token,
                user: p.kp.pubkey(),
                token_program: spl_token::ID,
            }
            .to_account_metas(None),
            data: ix::Unstake { amount: menge }.data(),
        }
    }

    fn govern_ix(&self, wer: &Pubkey, data: Vec<u8>) -> Instruction {
        Instruction {
            program_id: terrasol::ID,
            accounts: konten::Govern { config: self.config, governance: *wer }.to_account_metas(None),
            data,
        }
    }

    fn impact_pda(&self, subjekt: &Pubkey, index: u64) -> Pubkey {
        Pubkey::find_program_address(&[b"impact", subjekt.as_ref(), &index.to_le_bytes()], &terrasol::ID).0
    }

    async fn impact_registrieren(&mut self, subjekt: &Pubkey, uri: &str) -> Result<Pubkey, BanksClientError> {
        let index = self.config_lesen().await.impact_count;
        let impact = self.impact_pda(subjekt, index);
        let oracle = self.oracle.insecure_clone();
        let i = Instruction {
            program_id: terrasol::ID,
            accounts: konten::RegisterImpact {
                config: self.config,
                impact,
                oracle: oracle.pubkey(),
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: ix::RegisterImpact {
                subject: *subjekt,
                co2e_grams: 800_000_000,
                evidence_hash: [7u8; 32],
                uri: uri.to_string(),
            }
            .data(),
        };
        self.senden(&[i], &[&oracle]).await.map(|_| impact)
    }

    fn listing_pda(impact: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(&[b"listing", impact.as_ref()], &terrasol::ID).0
    }

    fn list_ix(&self, impact: &Pubkey, verkaeufer: &Pubkey, preis: u64) -> Instruction {
        Instruction {
            program_id: terrasol::ID,
            accounts: konten::ListCredit {
                config: self.config,
                impact: *impact,
                listing: Self::listing_pda(impact),
                seller: *verkaeufer,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: ix::ListCredit { price: preis }.data(),
        }
    }

    fn buy_ix(&self, impact: &Pubkey, kaeufer: &Person, verkaeufer_token: &Pubkey, max_preis: u64) -> Instruction {
        Instruction {
            program_id: terrasol::ID,
            accounts: konten::BuyCredit {
                config: self.config,
                listing: Self::listing_pda(impact),
                buyer_token: kaeufer.token,
                seller_token: *verkaeufer_token,
                buyer: kaeufer.kp.pubkey(),
                token_program: spl_token::ID,
            }
            .to_account_metas(None),
            data: ix::BuyCredit { max_price: max_preis }.data(),
        }
    }

    fn cancel_ix(&self, impact: &Pubkey, verkaeufer: &Pubkey) -> Instruction {
        Instruction {
            program_id: terrasol::ID,
            accounts: konten::CancelListing { listing: Self::listing_pda(impact), seller: *verkaeufer }
                .to_account_metas(None),
            data: ix::CancelListing {}.data(),
        }
    }

    async fn lesen<T: AccountDeserialize>(&mut self, adresse: Pubkey) -> T {
        let konto = self.ctx.banks_client.get_account(adresse).await.unwrap().expect("Konto fehlt");
        T::try_deserialize(&mut konto.data.as_slice()).unwrap()
    }

    async fn config_lesen(&mut self) -> Config {
        let c = self.config;
        self.lesen(c).await
    }

    async fn bestand(&mut self, token: Pubkey) -> u64 {
        let konto = self.ctx.banks_client.get_account(token).await.unwrap().unwrap();
        spl_token::state::Account::unpack(&konto.data).unwrap().amount
    }

    async fn uhr_vor(&mut self, sekunden: i64) {
        let mut uhr: Clock = self.ctx.banks_client.get_sysvar().await.unwrap();
        uhr.unix_timestamp += sekunden;
        self.ctx.set_sysvar(&uhr);
    }
}

// ===========================================================================
// initialize
// ===========================================================================

#[tokio::test]
async fn nur_upgrade_autoritaet_darf_initialisieren() {
    let mut w = Welt::neu().await;
    let fremder = Keypair::new();
    w.sol(&fremder.pubkey(), 10_000_000_000).await;
    let i = w.init_ix(&fremder.pubkey());
    let e = w.senden(&[i], &[&fremder]).await.unwrap_err();
    assert_eq!(fehlercode(e), Some(fehler(TerraError::UnauthorizedInitializer)));

    w.initialisieren().await;
    let cfg = w.config_lesen().await;
    assert_eq!(cfg.governance, w.governance.pubkey());
    assert_eq!(cfg.oracle, w.oracle.pubkey());
    assert_eq!(cfg.stake_mint, w.mint.pubkey());
    assert_eq!(cfg.tier_thresholds, STUFEN);
    assert_eq!(cfg.pending_governance, Pubkey::default());

    // Zweites initialize scheitert: die Config existiert bereits.
    let admin = w.admin.insecure_clone();
    let i = w.init_ix(&admin.pubkey());
    assert!(w.senden(&[i], &[&admin]).await.is_err());
}

// ===========================================================================
// stake / unstake
// ===========================================================================

#[tokio::test]
async fn unstake_nach_der_sperrfrist_zahlt_alles_zurueck() {
    let mut w = Welt::neu().await;
    w.initialisieren().await;
    let alice = w.person(5_000).await;

    let i = w.stake_ix(&alice, 1_500);
    w.senden(&[i], &[&alice.kp]).await.unwrap();
    assert_eq!(w.bestand(alice.token).await, 3_500);
    assert_eq!(w.bestand(w.vault).await, 1_500);
    let pos: StakePosition = w.lesen(w.position(&alice.kp.pubkey())).await;
    assert_eq!(pos.amount, 1_500);

    // Innerhalb der Sperrfrist: abgewiesen.
    let i = w.unstake_ix(&alice, 500);
    let e = w.senden(&[i], &[&alice.kp]).await.unwrap_err();
    assert_eq!(fehlercode(e), Some(fehler(TerraError::StillLocked)));

    // Nach 8 Tagen: Teilbetrag, dann der Rest.
    w.uhr_vor(8 * TAG).await;
    let i = w.unstake_ix(&alice, 500);
    w.senden(&[i], &[&alice.kp]).await.unwrap();
    assert_eq!(w.bestand(alice.token).await, 4_000);
    let i = w.unstake_ix(&alice, 1_000);
    w.senden(&[i], &[&alice.kp]).await.unwrap();
    assert_eq!(w.bestand(alice.token).await, 5_000, "Principal in == principal out");
    assert_eq!(w.bestand(w.vault).await, 0);
    assert_eq!(w.config_lesen().await.total_staked, 0);

    // Mehr als gestaked: abgewiesen.
    let i = w.unstake_ix(&alice, 1);
    let e = w.senden(&[i], &[&alice.kp]).await.unwrap_err();
    assert_eq!(fehlercode(e), Some(fehler(TerraError::InsufficientStake)));
}

#[tokio::test]
async fn pause_stoppt_stake_aber_nie_die_rueckzahlung() {
    let mut w = Welt::neu().await;
    w.initialisieren().await;
    let alice = w.person(1_000).await;
    let i = w.stake_ix(&alice, 1_000);
    w.senden(&[i], &[&alice.kp]).await.unwrap();

    let gov = w.governance.insecure_clone();
    let i = w.govern_ix(&gov.pubkey(), ix::SetPaused { paused: true }.data());
    w.senden(&[i], &[&gov]).await.unwrap();

    let bob = w.person(100).await;
    let i = w.stake_ix(&bob, 100);
    let e = w.senden(&[i], &[&bob.kp]).await.unwrap_err();
    assert_eq!(fehlercode(e), Some(fehler(TerraError::Paused)));

    w.uhr_vor(8 * TAG).await;
    let i = w.unstake_ix(&alice, 1_000);
    w.senden(&[i], &[&alice.kp]).await.unwrap();
    assert_eq!(w.bestand(alice.token).await, 1_000);
}

#[tokio::test]
async fn fremdes_konto_und_nullbetrag_abgewiesen() {
    let mut w = Welt::neu().await;
    w.initialisieren().await;
    let alice = w.person(1_000).await;
    let bob = w.person(1_000).await;

    let i = w.stake_ix(&alice, 0);
    let e = w.senden(&[i], &[&alice.kp]).await.unwrap_err();
    assert_eq!(fehlercode(e), Some(fehler(TerraError::ZeroAmount)));

    // Alice versucht, von Bobs Tokenkonto zu staken.
    let mut i = w.stake_ix(&alice, 10);
    i.accounts[3].pubkey = bob.token;
    let e = w.senden(&[i], &[&alice.kp]).await.unwrap_err();
    assert_eq!(fehlercode(e), Some(fehler(TerraError::WrongOwner)));
}

// ===========================================================================
// register_impact
// ===========================================================================

#[tokio::test]
async fn nur_das_oracle_registriert_impact() {
    let mut w = Welt::neu().await;
    w.initialisieren().await;
    let alice = w.person(0).await;

    // Fremder Signierer an Stelle des Oracles.
    let impact = w.impact_pda(&alice.kp.pubkey(), 0);
    let i = Instruction {
        program_id: terrasol::ID,
        accounts: konten::RegisterImpact {
            config: w.config,
            impact,
            oracle: alice.kp.pubkey(),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: ix::RegisterImpact {
            subject: alice.kp.pubkey(),
            co2e_grams: 1,
            evidence_hash: [0u8; 32],
            uri: String::new(),
        }
        .data(),
    };
    let e = w.senden(&[i], &[&alice.kp]).await.unwrap_err();
    assert_eq!(fehlercode(e), Some(fehler(TerraError::UnauthorizedOracle)));

    let zu_lang = "x".repeat(201);
    let e = w.impact_registrieren(&alice.kp.pubkey(), &zu_lang).await.unwrap_err();
    assert_eq!(fehlercode(e), Some(fehler(TerraError::UriTooLong)));

    w.impact_registrieren(&alice.kp.pubkey(), "https://terrasols.org/proof/0").await.unwrap();
    assert_eq!(w.config_lesen().await.impact_count, 1);
}

// ===========================================================================
// Marktplatz
// ===========================================================================

#[tokio::test]
async fn marktplatz_listen_kaufen_und_schranken() {
    let mut w = Welt::neu().await;
    w.initialisieren().await;
    let alice = w.person(0).await;
    let bob = w.person(1_000).await;
    let impact = w.impact_registrieren(&alice.kp.pubkey(), "u").await.unwrap();

    // Nur das Subjekt darf listen.
    let i = w.list_ix(&impact, &bob.kp.pubkey(), 250);
    let e = w.senden(&[i], &[&bob.kp]).await.unwrap_err();
    assert_eq!(fehlercode(e), Some(fehler(TerraError::NotCreditOwner)));

    let i = w.list_ix(&impact, &alice.kp.pubkey(), 250);
    w.senden(&[i], &[&alice.kp]).await.unwrap();

    // Selbstkauf verboten.
    let i = w.buy_ix(&impact, &alice, &alice.token, 250);
    let e = w.senden(&[i], &[&alice.kp]).await.unwrap_err();
    assert_eq!(fehlercode(e), Some(fehler(TerraError::SelfPurchase)));

    // Preis über dem Limit des Käufers: abgewiesen, nichts bezahlt.
    let i = w.buy_ix(&impact, &bob, &alice.token, 249);
    let e = w.senden(&[i], &[&bob.kp]).await.unwrap_err();
    assert_eq!(fehlercode(e), Some(fehler(TerraError::PriceAboveLimit)));
    assert_eq!(w.bestand(bob.token).await, 1_000);

    let i = w.buy_ix(&impact, &bob, &alice.token, 250);
    w.senden(&[i], &[&bob.kp]).await.unwrap();
    assert_eq!(w.bestand(bob.token).await, 750);
    assert_eq!(w.bestand(alice.token).await, 250);
    let l: Listing = w.lesen(Welt::listing_pda(&impact)).await;
    assert!(l.sold);
    assert_eq!(l.buyer, bob.kp.pubkey());

    // Zweiter Kauf und Storno nach Verkauf: abgewiesen.
    let carol = w.person(1_000).await;
    let i = w.buy_ix(&impact, &carol, &alice.token, 250);
    let e = w.senden(&[i], &[&carol.kp]).await.unwrap_err();
    assert_eq!(fehlercode(e), Some(fehler(TerraError::AlreadySold)));
    let i = w.cancel_ix(&impact, &alice.kp.pubkey());
    let e = w.senden(&[i], &[&alice.kp]).await.unwrap_err();
    assert_eq!(fehlercode(e), Some(fehler(TerraError::AlreadySold)));
}

#[tokio::test]
async fn storno_nur_durch_verkaeufer() {
    let mut w = Welt::neu().await;
    w.initialisieren().await;
    let alice = w.person(0).await;
    let bob = w.person(0).await;
    let impact = w.impact_registrieren(&alice.kp.pubkey(), "u").await.unwrap();
    let i = w.list_ix(&impact, &alice.kp.pubkey(), 10);
    w.senden(&[i], &[&alice.kp]).await.unwrap();

    let i = w.cancel_ix(&impact, &bob.kp.pubkey());
    let e = w.senden(&[i], &[&bob.kp]).await.unwrap_err();
    assert_eq!(fehlercode(e), Some(fehler(TerraError::NotCreditOwner)));

    let i = w.cancel_ix(&impact, &alice.kp.pubkey());
    w.senden(&[i], &[&alice.kp]).await.unwrap();
    let weg = w.ctx.banks_client.get_account(Welt::listing_pda(&impact)).await.unwrap();
    assert!(weg.is_none(), "Listing geschlossen, Miete zurück");
}

// ===========================================================================
// Governance
// ===========================================================================

#[tokio::test]
async fn governance_schranken_und_parameter() {
    let mut w = Welt::neu().await;
    w.initialisieren().await;
    let gov = w.governance.insecure_clone();
    let fremder = w.person(0).await;

    for data in [
        ix::SetPaused { paused: true }.data(),
        ix::SetOracle { new_oracle: fremder.kp.pubkey() }.data(),
        ix::SetThresholds { thresholds: [1, 2, 3, 4] }.data(),
        ix::SetGovernance { new_governance: fremder.kp.pubkey() }.data(),
    ] {
        let i = w.govern_ix(&fremder.kp.pubkey(), data);
        let e = w.senden(&[i], &[&fremder.kp]).await.unwrap_err();
        assert_eq!(fehlercode(e), Some(fehler(TerraError::UnauthorizedGovernance)));
    }

    let i = w.govern_ix(&gov.pubkey(), ix::SetThresholds { thresholds: [5, 4, 3, 2] }.data());
    let e = w.senden(&[i], &[&gov]).await.unwrap_err();
    assert_eq!(fehlercode(e), Some(fehler(TerraError::ThresholdsNotIncreasing)));

    let i = w.govern_ix(&gov.pubkey(), ix::SetThresholds { thresholds: [1, 2, 3, 4] }.data());
    w.senden(&[i], &[&gov]).await.unwrap();
    let neues_oracle = Keypair::new();
    let i = w.govern_ix(&gov.pubkey(), ix::SetOracle { new_oracle: neues_oracle.pubkey() }.data());
    w.senden(&[i], &[&gov]).await.unwrap();
    let cfg = w.config_lesen().await;
    assert_eq!(cfg.tier_thresholds, [1, 2, 3, 4]);
    assert_eq!(cfg.oracle, neues_oracle.pubkey());
}

#[tokio::test]
async fn governance_wechsel_in_zwei_schritten() {
    let mut w = Welt::neu().await;
    w.initialisieren().await;
    let gov = w.governance.insecure_clone();
    let neu = w.person(0).await;
    let fremder = w.person(0).await;

    let annehmen = |wer: &Pubkey, config: Pubkey| Instruction {
        program_id: terrasol::ID,
        accounts: konten::AcceptGovernance { config, new_governance: *wer }.to_account_metas(None),
        data: ix::AcceptGovernance {}.data(),
    };

    // Ohne Vorschlag gibt es nichts anzunehmen.
    let i = annehmen(&neu.kp.pubkey(), w.config);
    let e = w.senden(&[i], &[&neu.kp]).await.unwrap_err();
    assert_eq!(fehlercode(e), Some(fehler(TerraError::NoPendingGovernance)));

    let i = w.govern_ix(&gov.pubkey(), ix::SetGovernance { new_governance: Pubkey::default() }.data());
    let e = w.senden(&[i], &[&gov]).await.unwrap_err();
    assert_eq!(fehlercode(e), Some(fehler(TerraError::InvalidAuthority)));

    let i = w.govern_ix(&gov.pubkey(), ix::SetGovernance { new_governance: neu.kp.pubkey() }.data());
    w.senden(&[i], &[&gov]).await.unwrap();
    let cfg = w.config_lesen().await;
    assert_eq!(cfg.governance, gov.pubkey(), "noch nichts übertragen");
    assert_eq!(cfg.pending_governance, neu.kp.pubkey());

    // Nur der vorgeschlagene Schlüssel kann annehmen.
    let i = annehmen(&fremder.kp.pubkey(), w.config);
    let e = w.senden(&[i], &[&fremder.kp]).await.unwrap_err();
    assert_eq!(fehlercode(e), Some(fehler(TerraError::UnauthorizedGovernance)));

    let i = annehmen(&neu.kp.pubkey(), w.config);
    w.senden(&[i], &[&neu.kp]).await.unwrap();
    let cfg = w.config_lesen().await;
    assert_eq!(cfg.governance, neu.kp.pubkey());
    assert_eq!(cfg.pending_governance, Pubkey::default());

    // Die alte Governance hat keine Rechte mehr.
    let i = w.govern_ix(&gov.pubkey(), ix::SetPaused { paused: true }.data());
    let e = w.senden(&[i], &[&gov]).await.unwrap_err();
    assert_eq!(fehlercode(e), Some(fehler(TerraError::UnauthorizedGovernance)));
}
