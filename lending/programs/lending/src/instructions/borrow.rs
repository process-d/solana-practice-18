use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token_interface::{ self, Mint, TokenAccount, TokenInterface, TransferChecked };
use crate::constants::{SOL_USD_FEED_ID, USDC_USD_FEED_ID};
use crate::state::*;
use crate::error::ErrorCode;

/// 直接从 Pyth Price Account 读取价格 (使用 Anchor 内置方式)
/// Pyth Price Account 布局:
/// - expo: offset 20-24 (i32)
/// - price: offset 108-116 (i64)
fn read_pyth_price(price_account: &UncheckedAccount) -> Result<(u64, i32)> {
    let data = price_account.data.borrow();
    
    require!(data.len() > 116, ErrorCode::InvalidPrice);
    
    // 读取指数 (offset 20-24)
    let expo = i32::from_le_bytes(
        data[20..24].try_into().map_err(|_| ErrorCode::InvalidPrice)?
    );
    
    // 读取价格 (offset 108-116)
    let price = i64::from_le_bytes(
        data[108..116].try_into().map_err(|_| ErrorCode::InvalidPrice)?
    );
    
    if price == 0 {
        return Err(ErrorCode::InvalidPrice.into());
    }
    
    Ok((price.unsigned_abs() as u64, expo))
}

#[derive(Accounts)]
pub struct Borrow<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
    pub mint: InterfaceAccount<'info, Mint>,
    #[account(
        mut, 
        seeds = [mint.key().as_ref()],
        bump,
    )]  
    pub bank: Account<'info, Bank>,
    #[account(
        mut, 
        seeds = [b"treasury", mint.key().as_ref()],
        bump, 
    )]  
    pub bank_token_account: InterfaceAccount<'info, TokenAccount>,
    #[account(
        mut, 
        seeds = [signer.key().as_ref()],
        bump,
    )]  
    pub user_account: Account<'info, User>,
    #[account( 
        init_if_needed, 
        payer = signer,
        associated_token::mint = mint, 
        associated_token::authority = signer,
        associated_token::token_program = token_program,
    )]
    pub user_token_account: InterfaceAccount<'info, TokenAccount>, 
    /// Pyth SOL/USD price account
    pub sol_price_account: UncheckedAccount<'info>,
    /// Pyth USDC/USD price account
    pub usdc_price_account: UncheckedAccount<'info>,
    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn process_borrow(ctx: Context<Borrow>, amount: u64) -> Result<()> {
    let bank = &mut ctx.accounts.bank;
    let user = &mut ctx.accounts.user_account;

    let (sol_price, sol_expo) = read_pyth_price(&ctx.accounts.sol_price_account)?;
    let (usdc_price, usdc_expo) = read_pyth_price(&ctx.accounts.usdc_price_account)?;

    // 应用指数转换为标准单位 (8位小数)
    let sol_price_wei = sol_price * 10_u64.pow((-sol_expo) as u32);
    let usdc_price_wei = usdc_price * 10_u64.pow((-usdc_expo) as u32);

    let total_collateral: u64;

    match ctx.accounts.mint.to_account_info().key() {
        key if key == user.usdc_address => {
            let accrued_interest = calculate_accrued_interest(user.deposited_sol, bank.interest_rate, user.last_updated)?;
            total_collateral = sol_price_wei * (user.deposited_sol + accrued_interest) / sol_price_wei;
        },
        _ => {
            total_collateral = usdc_price_wei * user.deposited_usdc / usdc_price_wei;
        }
    }

    let borrowable_amount = total_collateral * bank.liquidation_threshold / 10000;

    if borrowable_amount < amount {
        return Err(ErrorCode::OverBorrowableAmount.into());
    }       

    let transfer_cpi_accounts = TransferChecked {
        from: ctx.accounts.bank_token_account.to_account_info(),
        mint: ctx.accounts.mint.to_account_info(),
        to: ctx.accounts.user_token_account.to_account_info(),
        authority: ctx.accounts.bank_token_account.to_account_info(),
    };

    let cpi_program = ctx.accounts.token_program.to_account_info();
    let mint_key = ctx.accounts.mint.key();
    let signer_seeds: &[&[&[u8]]] = &[
        &[
            b"treasury",
            mint_key.as_ref(),
            &[ctx.bumps.bank_token_account],
        ],
    ];
    let cpi_ctx = CpiContext::new(cpi_program, transfer_cpi_accounts).with_signer(signer_seeds);
    let decimals = ctx.accounts.mint.decimals;

    token_interface::transfer_checked(cpi_ctx, amount, decimals)?;

    if bank.total_borrowed == 0 {
        bank.total_borrowed = amount;
        bank.total_borrowed_shares = amount;
    } 

    let borrow_ratio = amount.checked_div(bank.total_borrowed).unwrap_or(0);
    let users_shares = bank.total_borrowed_shares.checked_mul(borrow_ratio).unwrap_or(0);

    bank.total_borrowed += amount;
    bank.total_borrowed_shares += users_shares; 

    let accrued_interest = calculate_accrued_interest(user.deposited_sol, bank.interest_rate, user.last_updated)?;
    if accrued_interest > user.deposited_sol {
        let interest_earned = accrued_interest - user.deposited_sol;
        bank.total_deposits += interest_earned;
    }

    match ctx.accounts.mint.to_account_info().key() {
        key if key == user.usdc_address => {
            user.borrowed_usdc += amount;
            user.deposited_usdc_shares += users_shares;
        },
        _ => {
            user.borrowed_sol += amount;
            user.borrowed_sol_shares += users_shares;
        }
    }

    Ok(())
}

fn calculate_accrued_interest(deposited: u64, interest_rate: u64, last_update: i64) -> Result<u64> {
    let current_time = Clock::get()?.unix_timestamp;
    let time_elapsed = current_time - last_update;
    if time_elapsed <= 0 {
        return Ok(deposited);
    }
    let rate_per_second = interest_rate as f64 / (365.0 * 24.0 * 3600.0 * 10000.0);
    let multiplier = 1.0 + rate_per_second * time_elapsed as f64;
    let new_value = (deposited as f64 * multiplier) as u64;
    Ok(new_value)
}