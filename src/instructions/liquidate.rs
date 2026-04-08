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
    
    let expo = i32::from_le_bytes(
        data[20..24].try_into().map_err(|_| ErrorCode::InvalidPrice)?
    );
    
    let price = i64::from_le_bytes(
        data[108..116].try_into().map_err(|_| ErrorCode::InvalidPrice)?
    );
    
    if price == 0 {
        return Err(ErrorCode::InvalidPrice.into());
    }
    
    Ok((price.unsigned_abs() as u64, expo))
}

#[derive(Accounts)]
pub struct Liquidate<'info> {
    #[account(mut)]
    pub liquidator: Signer<'info>,
    pub collateral_mint: InterfaceAccount<'info, Mint>,
    pub borrowed_mint: InterfaceAccount<'info, Mint>,
    #[account(
        mut, 
        seeds = [collateral_mint.key().as_ref()],
        bump,
    )]  
    pub collateral_bank: Account<'info, Bank>,
    #[account(
        mut, 
        seeds = [b"treasury", collateral_mint.key().as_ref()],
        bump, 
    )]  
    pub collateral_bank_token_account: InterfaceAccount<'info, TokenAccount>,
    #[account(
        mut, 
        seeds = [borrowed_mint.key().as_ref()],
        bump,
    )]  
    pub borrowed_bank: Account<'info, Bank>,
    #[account(
        mut, 
        seeds = [b"treasury", borrowed_mint.key().as_ref()],
        bump, 
    )]  
    pub borrowed_bank_token_account: InterfaceAccount<'info, TokenAccount>,
    #[account(
        mut, 
        seeds = [liquidator.key().as_ref()],
        bump,
    )]  
    pub user_account: Account<'info, User>,
    #[account( 
        init_if_needed, 
        payer = liquidator,
        associated_token::mint = collateral_mint, 
        associated_token::authority = liquidator,
        associated_token::token_program = token_program,
    )]
    pub liquidator_collateral_token_account: InterfaceAccount<'info, TokenAccount>, 
    #[account( 
        init_if_needed, 
        payer = liquidator,
        associated_token::mint = borrowed_mint, 
        associated_token::authority = liquidator,
        associated_token::token_program = token_program,
    )]
    pub liquidator_borrowed_token_account: InterfaceAccount<'info, TokenAccount>, 
    pub sol_price_account: UncheckedAccount<'info>,
    pub usdc_price_account: UncheckedAccount<'info>,
    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn process_liquidate(ctx: Context<Liquidate>) -> Result<()> { 
    let collateral_bank = &mut ctx.accounts.collateral_bank;
    let user = &mut ctx.accounts.user_account;

    let (sol_price, sol_expo) = read_pyth_price(&ctx.accounts.sol_price_account)?;
    let (usdc_price, usdc_expo) = read_pyth_price(&ctx.accounts.usdc_price_account)?;

    let sol_price_wei = sol_price * 10_u64.pow((-sol_expo) as u32);
    let usdc_price_wei = usdc_price * 10_u64.pow((-usdc_expo) as u32);

    let total_collateral = sol_price_wei * user.deposited_sol / sol_price_wei
        + usdc_price_wei * user.deposited_usdc / usdc_price_wei;
    let total_borrowed = sol_price_wei * user.borrowed_sol / sol_price_wei 
        + usdc_price_wei * user.borrowed_usdc / usdc_price_wei;    

    let health_factor = if total_borrowed > 0 {
        (total_collateral * 10000) / total_borrowed
    } else {
        10000
    };

    if health_factor >= collateral_bank.liquidation_threshold {
        return Err(ErrorCode::NotUndercollateralized.into());
    }

    let liquidation_amount = total_borrowed * collateral_bank.liquidation_close_factor / 10000;

    let transfer_to_bank = TransferChecked {
        from: ctx.accounts.liquidator_borrowed_token_account.to_account_info(),
        mint: ctx.accounts.borrowed_mint.to_account_info(),
        to: ctx.accounts.borrowed_bank_token_account.to_account_info(),
        authority: ctx.accounts.liquidator.to_account_info(),
    };

    let cpi_program = ctx.accounts.token_program.to_account_info();
    let cpi_ctx_to_bank = CpiContext::new(cpi_program.clone(), transfer_to_bank);
    let decimals = ctx.accounts.borrowed_mint.decimals;

    token_interface::transfer_checked(cpi_ctx_to_bank, liquidation_amount, decimals)?;

    let liquidation_bonus = (liquidation_amount * collateral_bank.liquidation_bonus) / 10000 + liquidation_amount;
    
    let transfer_to_liquidator = TransferChecked {
        from: ctx.accounts.collateral_bank_token_account.to_account_info(),
        mint: ctx.accounts.collateral_mint.to_account_info(),
        to: ctx.accounts.liquidator_collateral_token_account.to_account_info(),
        authority: ctx.accounts.collateral_bank_token_account.to_account_info(),
    };

    let mint_key = ctx.accounts.collateral_mint.key();
    let signer_seeds: &[&[&[u8]]] = &[
        &[
            b"treasury",
            mint_key.as_ref(),
            &[ctx.bumps.collateral_bank_token_account],
        ],
    ];
    let cpi_ctx_to_liquidator = CpiContext::new(cpi_program.clone(), transfer_to_liquidator).with_signer(signer_seeds);
    let collateral_decimals = ctx.accounts.collateral_mint.decimals;   
    token_interface::transfer_checked(cpi_ctx_to_liquidator, liquidation_bonus, collateral_decimals)?;

    let usdc_address = user.usdc_address;
    
    let borrowed_mint_key = ctx.accounts.borrowed_mint.to_account_info().key();
    if borrowed_mint_key == usdc_address {
        user.borrowed_usdc = user.borrowed_usdc.saturating_sub(liquidation_amount);
    } else {
        user.borrowed_sol = user.borrowed_sol.saturating_sub(liquidation_amount);
    }
    
    let collateral_mint_key = ctx.accounts.collateral_mint.to_account_info().key();
    if collateral_mint_key == usdc_address {
        user.deposited_usdc = user.deposited_usdc.saturating_sub(liquidation_bonus);
    } else {
        user.deposited_sol = user.deposited_sol.saturating_sub(liquidation_bonus);
    }
    
    let borrowed_bank = &mut ctx.accounts.borrowed_bank;
    borrowed_bank.total_borrowed = borrowed_bank.total_borrowed.saturating_sub(liquidation_amount);
    
    collateral_bank.total_deposits = collateral_bank.total_deposits.saturating_sub(liquidation_bonus);
    
    let new_total_collateral = sol_price_wei * user.deposited_sol / sol_price_wei
        + usdc_price_wei * user.deposited_usdc / usdc_price_wei;
    let new_total_borrowed = sol_price_wei * user.borrowed_sol / sol_price_wei 
        + usdc_price_wei * user.borrowed_usdc / usdc_price_wei;
    if new_total_borrowed > 0 {
        user.health_factor = (new_total_collateral * 10000) / new_total_borrowed;
    } else {
        user.health_factor = 10000;
    }
    
    Ok(())
}