//! TerraSol on-chain program (Anchor).
//!
//! Design goals (legal + security):
//! - TRRA is a pure UTILITY token: staking grants ACCESS TIERS, never yield.
//! - Proof-of-Impact records are oracle-signed, append-only attestations.
//! - Marketplace: verified impact credits can be listed and sold for TRRA. The
//!   attestation stays immutable; tradeable ownership lives in a Listing account.
//! - Governance is delegated to SPL-Governance / Realms.

use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};

declare_id!("3GGT5oAJXjpvFnofn3W25jTBhKRp4TEmKSSyzm7J7E9z");

const MAX_TIER: u8 = 4;
const MIN_LOCK_SECONDS: i64 = 7 * 24 * 60 * 60;
const MAX_URI_LEN: usize = 200;

#[program]
pub mod terrasol {
    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        tier_thresholds: [u64; MAX_TIER as usize],
    ) -> Result<()> {
        let mut prev: u64 = 0;
        for (i, t) in tier_thresholds.iter().enumerate() {
            require!(*t >= prev || i == 0, TerraError::ThresholdsNotIncreasing);
            prev = *t;
        }
        let cfg = &mut ctx.accounts.config;
        cfg.governance = ctx.accounts.governance.key();
        cfg.oracle = ctx.accounts.oracle.key();
        cfg.stake_mint = ctx.accounts.stake_mint.key();
        cfg.vault = ctx.accounts.vault.key();
        cfg.tier_thresholds = tier_thresholds;
        cfg.total_staked = 0;
        cfg.impact_count = 0;
        cfg.paused = false;
        cfg.pending_governance = Pubkey::default();
        cfg.bump = ctx.bumps.config;
        cfg.vault_bump = ctx.bumps.vault;
        emit!(Initialized { governance: cfg.governance, oracle: cfg.oracle, stake_mint: cfg.stake_mint });
        Ok(())
    }

    pub fn stake(ctx: Context<Stake>, amount: u64) -> Result<()> {
        let cfg = &ctx.accounts.config;
        require!(!cfg.paused, TerraError::Paused);
        require!(amount > 0, TerraError::ZeroAmount);
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.user_token.to_account_info(),
                    to: ctx.accounts.vault.to_account_info(),
                    authority: ctx.accounts.user.to_account_info(),
                },
            ),
            amount,
        )?;
        let pos = &mut ctx.accounts.position;
        let clock = Clock::get()?;
        pos.owner = ctx.accounts.user.key();
        pos.amount = pos.amount.checked_add(amount).ok_or(TerraError::MathOverflow)?;
        pos.locked_until = clock.unix_timestamp.checked_add(MIN_LOCK_SECONDS).ok_or(TerraError::MathOverflow)?;
        pos.bump = ctx.bumps.position;
        let cfg_mut = &mut ctx.accounts.config;
        cfg_mut.total_staked = cfg_mut.total_staked.checked_add(amount).ok_or(TerraError::MathOverflow)?;
        let tier = tier_for(&cfg_mut.tier_thresholds, pos.amount);
        emit!(Staked { owner: pos.owner, amount, total: pos.amount, tier });
        Ok(())
    }

    /// Returns staked principal. Deliberately NOT gated by `paused`: the pause
    /// stops new activity, it must never freeze a holder's own principal
    /// (SECURITY.md invariant #1, utility-token design).
    pub fn unstake(ctx: Context<Unstake>, amount: u64) -> Result<()> {
        require!(amount > 0, TerraError::ZeroAmount);
        let pos = &mut ctx.accounts.position;
        require!(pos.amount >= amount, TerraError::InsufficientStake);
        let clock = Clock::get()?;
        require!(clock.unix_timestamp >= pos.locked_until, TerraError::StillLocked);
        // The vault's token authority is the CONFIG PDA (see `Initialize`), so the
        // CPI must be signed with the config seeds - not with the vault's own.
        let config_bump = ctx.accounts.config.bump;
        let seeds = &[b"config".as_ref(), &[config_bump]];
        let signer = &[&seeds[..]];
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.vault.to_account_info(),
                    to: ctx.accounts.user_token.to_account_info(),
                    authority: ctx.accounts.config.to_account_info(),
                },
                signer,
            ),
            amount,
        )?;
        pos.amount = pos.amount.checked_sub(amount).ok_or(TerraError::MathOverflow)?;
        let cfg_mut = &mut ctx.accounts.config;
        cfg_mut.total_staked = cfg_mut.total_staked.checked_sub(amount).ok_or(TerraError::MathOverflow)?;
        let tier = tier_for(&cfg_mut.tier_thresholds, pos.amount);
        emit!(Unstaked { owner: pos.owner, amount, remaining: pos.amount, tier });
        Ok(())
    }

    pub fn register_impact(
        ctx: Context<RegisterImpact>,
        subject: Pubkey,
        co2e_grams: u64,
        evidence_hash: [u8; 32],
        uri: String,
    ) -> Result<()> {
        let cfg = &ctx.accounts.config;
        require!(!cfg.paused, TerraError::Paused);
        require!(uri.len() <= MAX_URI_LEN, TerraError::UriTooLong);
        require!(co2e_grams > 0, TerraError::ZeroAmount);
        let rec = &mut ctx.accounts.impact;
        rec.subject = subject;
        rec.oracle = ctx.accounts.oracle.key();
        rec.co2e_grams = co2e_grams;
        rec.evidence_hash = evidence_hash;
        rec.uri = uri;
        rec.timestamp = Clock::get()?.unix_timestamp;
        rec.index = cfg.impact_count;
        rec.bump = ctx.bumps.impact;
        let cfg_mut = &mut ctx.accounts.config;
        cfg_mut.impact_count = cfg_mut.impact_count.checked_add(1).ok_or(TerraError::MathOverflow)?;
        emit!(ImpactRegistered { subject, oracle: rec.oracle, co2e_grams, index: rec.index });
        Ok(())
    }

    pub fn set_oracle(ctx: Context<Govern>, new_oracle: Pubkey) -> Result<()> {
        ctx.accounts.config.oracle = new_oracle;
        emit!(OracleRotated { new_oracle });
        Ok(())
    }

    pub fn set_thresholds(ctx: Context<Govern>, thresholds: [u64; MAX_TIER as usize]) -> Result<()> {
        let mut prev: u64 = 0;
        for (i, t) in thresholds.iter().enumerate() {
            require!(*t >= prev || i == 0, TerraError::ThresholdsNotIncreasing);
            prev = *t;
        }
        ctx.accounts.config.tier_thresholds = thresholds;
        Ok(())
    }

    pub fn set_paused(ctx: Context<Govern>, paused: bool) -> Result<()> {
        ctx.accounts.config.paused = paused;
        emit!(PauseToggled { paused });
        Ok(())
    }

    /// Step 1 of 2: propose a new governance authority. Nothing changes until
    /// the proposed key accepts, so a mistyped key can never capture control.
    pub fn set_governance(ctx: Context<Govern>, new_governance: Pubkey) -> Result<()> {
        require!(new_governance != Pubkey::default(), TerraError::InvalidAuthority);
        ctx.accounts.config.pending_governance = new_governance;
        emit!(GovernanceProposed { proposed: new_governance });
        Ok(())
    }

    /// Step 2 of 2: the proposed authority signs and takes over.
    pub fn accept_governance(ctx: Context<AcceptGovernance>) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        cfg.governance = cfg.pending_governance;
        cfg.pending_governance = Pubkey::default();
        emit!(GovernanceTransferred { new_governance: cfg.governance });
        Ok(())
    }

    /// List a verified impact credit for sale. Only the credit's subject may list.
    pub fn list_credit(ctx: Context<ListCredit>, price: u64) -> Result<()> {
        let cfg = &ctx.accounts.config;
        require!(!cfg.paused, TerraError::Paused);
        require!(price > 0, TerraError::ZeroAmount);
        require!(ctx.accounts.impact.subject == ctx.accounts.seller.key(), TerraError::NotCreditOwner);
        let listing = &mut ctx.accounts.listing;
        listing.seller = ctx.accounts.seller.key();
        listing.impact = ctx.accounts.impact.key();
        listing.impact_index = ctx.accounts.impact.index;
        listing.payment_mint = cfg.stake_mint;
        listing.price = price;
        listing.buyer = Pubkey::default();
        listing.sold = false;
        listing.bump = ctx.bumps.listing;
        emit!(CreditListed { listing: listing.key(), seller: listing.seller, impact_index: listing.impact_index, price });
        Ok(())
    }

    /// Buy a listed credit. Buyer pays `price` TRRA to the seller, but never
    /// more than `max_price`: a listing cancelled and re-listed at a higher
    /// price between signing and execution makes the purchase fail instead.
    pub fn buy_credit(ctx: Context<BuyCredit>, max_price: u64) -> Result<()> {
        let cfg = &ctx.accounts.config;
        require!(!cfg.paused, TerraError::Paused);
        require!(!ctx.accounts.listing.sold, TerraError::AlreadySold);
        require!(ctx.accounts.buyer.key() != ctx.accounts.listing.seller, TerraError::SelfPurchase);
        let price = ctx.accounts.listing.price;
        require!(price <= max_price, TerraError::PriceAboveLimit);
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.buyer_token.to_account_info(),
                    to: ctx.accounts.seller_token.to_account_info(),
                    authority: ctx.accounts.buyer.to_account_info(),
                },
            ),
            price,
        )?;
        let listing = &mut ctx.accounts.listing;
        listing.buyer = ctx.accounts.buyer.key();
        listing.sold = true;
        emit!(CreditSold { listing: listing.key(), seller: listing.seller, buyer: listing.buyer, impact_index: listing.impact_index, price });
        Ok(())
    }

    /// Cancel an unsold listing and reclaim rent. Seller only.
    pub fn cancel_listing(ctx: Context<CancelListing>) -> Result<()> {
        require!(!ctx.accounts.listing.sold, TerraError::AlreadySold);
        emit!(ListingCancelled { listing: ctx.accounts.listing.key(), seller: ctx.accounts.listing.seller });
        Ok(())
    }
}

fn tier_for(thresholds: &[u64; MAX_TIER as usize], staked: u64) -> u8 {
    let mut tier: u8 = 0;
    for (i, t) in thresholds.iter().enumerate() {
        if staked >= *t {
            tier = (i as u8) + 1;
        }
    }
    tier
}

#[account]
pub struct Config {
    pub governance: Pubkey,
    pub oracle: Pubkey,
    pub stake_mint: Pubkey,
    pub vault: Pubkey,
    pub tier_thresholds: [u64; MAX_TIER as usize],
    pub total_staked: u64,
    pub impact_count: u64,
    pub paused: bool,
    pub bump: u8,
    pub vault_bump: u8,
    /// Proposed governance authority; `Pubkey::default()` when none is pending.
    pub pending_governance: Pubkey,
}
impl Config {
    pub const LEN: usize = 8 + (32 * 4) + (8 * 4) + (8 * 2) + 1 + 1 + 1 + 32;
}

#[account]
pub struct StakePosition {
    pub owner: Pubkey,
    pub amount: u64,
    pub locked_until: i64,
    pub bump: u8,
}
impl StakePosition {
    pub const LEN: usize = 8 + 32 + 8 + 8 + 1;
}

#[account]
pub struct ImpactRecord {
    pub subject: Pubkey,
    pub oracle: Pubkey,
    pub co2e_grams: u64,
    pub evidence_hash: [u8; 32],
    pub uri: String,
    pub timestamp: i64,
    pub index: u64,
    pub bump: u8,
}
impl ImpactRecord {
    pub const LEN: usize = 8 + 32 + 32 + 8 + 32 + (4 + MAX_URI_LEN) + 8 + 8 + 1;
}

#[account]
pub struct Listing {
    pub seller: Pubkey,
    pub impact: Pubkey,
    pub impact_index: u64,
    pub payment_mint: Pubkey,
    pub price: u64,
    pub buyer: Pubkey,
    pub sold: bool,
    pub bump: u8,
}
impl Listing {
    pub const LEN: usize = 8 + 32 + 32 + 8 + 32 + 8 + 32 + 1 + 1;
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(init, payer = payer, space = Config::LEN, seeds = [b"config"], bump)]
    pub config: Account<'info, Config>,
    /// CHECK: stored as authority; validated by governance model off-chain.
    pub governance: UncheckedAccount<'info>,
    /// CHECK: stored as oracle authority; rotatable via set_oracle.
    pub oracle: UncheckedAccount<'info>,
    pub stake_mint: Account<'info, anchor_spl::token::Mint>,
    #[account(init, payer = payer, seeds = [b"vault", config.key().as_ref()], bump, token::mint = stake_mint, token::authority = config)]
    pub vault: Account<'info, TokenAccount>,
    #[account(mut)]
    pub payer: Signer<'info>,
    /// Only the program's upgrade authority may initialize. Without this check
    /// whoever calls first after the deploy picks governance, oracle and mint
    /// of the singleton config.
    #[account(
        seeds = [crate::ID.as_ref()],
        bump,
        seeds::program = anchor_lang::solana_program::bpf_loader_upgradeable::ID,
        constraint = program_data.upgrade_authority_address == Some(payer.key()) @ TerraError::UnauthorizedInitializer,
    )]
    pub program_data: Account<'info, ProgramData>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct Stake<'info> {
    #[account(mut, seeds = [b"config"], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(init_if_needed, payer = user, space = StakePosition::LEN, seeds = [b"position", user.key().as_ref()], bump)]
    pub position: Account<'info, StakePosition>,
    #[account(mut, address = config.vault)]
    pub vault: Account<'info, TokenAccount>,
    #[account(mut, constraint = user_token.mint == config.stake_mint @ TerraError::WrongMint, constraint = user_token.owner == user.key() @ TerraError::WrongOwner)]
    pub user_token: Account<'info, TokenAccount>,
    #[account(mut)]
    pub user: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Unstake<'info> {
    #[account(mut, seeds = [b"config"], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(mut, seeds = [b"position", user.key().as_ref()], bump = position.bump, constraint = position.owner == user.key() @ TerraError::WrongOwner)]
    pub position: Account<'info, StakePosition>,
    #[account(mut, address = config.vault)]
    pub vault: Account<'info, TokenAccount>,
    #[account(mut, constraint = user_token.mint == config.stake_mint @ TerraError::WrongMint, constraint = user_token.owner == user.key() @ TerraError::WrongOwner)]
    pub user_token: Account<'info, TokenAccount>,
    #[account(mut)]
    pub user: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
#[instruction(subject: Pubkey)]
pub struct RegisterImpact<'info> {
    #[account(mut, seeds = [b"config"], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(init, payer = oracle, space = ImpactRecord::LEN, seeds = [b"impact", subject.as_ref(), &config.impact_count.to_le_bytes()], bump)]
    pub impact: Account<'info, ImpactRecord>,
    #[account(mut, address = config.oracle @ TerraError::UnauthorizedOracle)]
    pub oracle: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Govern<'info> {
    #[account(mut, seeds = [b"config"], bump = config.bump, has_one = governance @ TerraError::UnauthorizedGovernance)]
    pub config: Account<'info, Config>,
    pub governance: Signer<'info>,
}

#[derive(Accounts)]
pub struct AcceptGovernance<'info> {
    #[account(
        mut,
        seeds = [b"config"],
        bump = config.bump,
        constraint = config.pending_governance != Pubkey::default() @ TerraError::NoPendingGovernance,
        constraint = config.pending_governance == new_governance.key() @ TerraError::UnauthorizedGovernance,
    )]
    pub config: Account<'info, Config>,
    pub new_governance: Signer<'info>,
}

#[derive(Accounts)]
pub struct ListCredit<'info> {
    #[account(seeds = [b"config"], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(seeds = [b"impact", impact.subject.as_ref(), &impact.index.to_le_bytes()], bump = impact.bump)]
    pub impact: Account<'info, ImpactRecord>,
    #[account(init, payer = seller, space = Listing::LEN, seeds = [b"listing", impact.key().as_ref()], bump)]
    pub listing: Account<'info, Listing>,
    #[account(mut)]
    pub seller: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct BuyCredit<'info> {
    #[account(seeds = [b"config"], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(mut, seeds = [b"listing", listing.impact.as_ref()], bump = listing.bump)]
    pub listing: Account<'info, Listing>,
    #[account(mut, constraint = buyer_token.mint == config.stake_mint @ TerraError::WrongMint, constraint = buyer_token.owner == buyer.key() @ TerraError::WrongOwner)]
    pub buyer_token: Account<'info, TokenAccount>,
    #[account(mut, constraint = seller_token.mint == config.stake_mint @ TerraError::WrongMint, constraint = seller_token.owner == listing.seller @ TerraError::WrongOwner)]
    pub seller_token: Account<'info, TokenAccount>,
    #[account(mut)]
    pub buyer: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct CancelListing<'info> {
    #[account(mut, close = seller, seeds = [b"listing", listing.impact.as_ref()], bump = listing.bump, has_one = seller @ TerraError::NotCreditOwner)]
    pub listing: Account<'info, Listing>,
    #[account(mut)]
    pub seller: Signer<'info>,
}

#[event]
pub struct Initialized { pub governance: Pubkey, pub oracle: Pubkey, pub stake_mint: Pubkey }
#[event]
pub struct Staked { pub owner: Pubkey, pub amount: u64, pub total: u64, pub tier: u8 }
#[event]
pub struct Unstaked { pub owner: Pubkey, pub amount: u64, pub remaining: u64, pub tier: u8 }
#[event]
pub struct ImpactRegistered { pub subject: Pubkey, pub oracle: Pubkey, pub co2e_grams: u64, pub index: u64 }
#[event]
pub struct OracleRotated { pub new_oracle: Pubkey }
#[event]
pub struct PauseToggled { pub paused: bool }
#[event]
pub struct GovernanceProposed { pub proposed: Pubkey }
#[event]
pub struct GovernanceTransferred { pub new_governance: Pubkey }
#[event]
pub struct CreditListed { pub listing: Pubkey, pub seller: Pubkey, pub impact_index: u64, pub price: u64 }
#[event]
pub struct CreditSold { pub listing: Pubkey, pub seller: Pubkey, pub buyer: Pubkey, pub impact_index: u64, pub price: u64 }
#[event]
pub struct ListingCancelled { pub listing: Pubkey, pub seller: Pubkey }

#[error_code]
pub enum TerraError {
    #[msg("Arithmetic overflow")]
    MathOverflow,
    #[msg("Amount must be greater than zero")]
    ZeroAmount,
    #[msg("Program is paused")]
    Paused,
    #[msg("Stake is still locked")]
    StillLocked,
    #[msg("Insufficient staked balance")]
    InsufficientStake,
    #[msg("Tier thresholds must be non-decreasing")]
    ThresholdsNotIncreasing,
    #[msg("Token account mint mismatch")]
    WrongMint,
    #[msg("Token account owner mismatch")]
    WrongOwner,
    #[msg("Impact URI too long")]
    UriTooLong,
    #[msg("Caller is not the authorised oracle")]
    UnauthorizedOracle,
    #[msg("Caller is not the governance authority")]
    UnauthorizedGovernance,
    #[msg("Invalid authority")]
    InvalidAuthority,
    #[msg("Caller does not own this credit")]
    NotCreditOwner,
    #[msg("Listing already sold")]
    AlreadySold,
    #[msg("Seller cannot buy own listing")]
    SelfPurchase,
    // New variants go at the end: error codes are 6000 + index.
    #[msg("Listing price is above the buyer's limit")]
    PriceAboveLimit,
    #[msg("Only the program's upgrade authority may initialize")]
    UnauthorizedInitializer,
    #[msg("No governance transfer is pending")]
    NoPendingGovernance,
}
