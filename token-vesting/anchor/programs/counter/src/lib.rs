#![allow(clippy::result_large_err)]

use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token_interface::{ self, Mint, TokenAccount, TokenInterface, TransferChecked };


declare_id!("GFdLg11UBR8ZeePW43ZyD1gY4z4UQ96LPa22YBgnn4z8");

#[program]
pub mod vesting {
    use super::*;
    // 根据已有信息创建一个
    pub fn create_vesting_account(ctx: Context<CreateVestingAccount>, company_name: String) -> Result<()> {
        *ctx.accounts.vesting_account = VestingAccount {
            owner: ctx.accounts.signer.key(),
            mint: ctx.accounts.mint.key(),
            treasury_token_account: ctx.accounts.treasury_token_account.key(),
            company_name,
            treasury_bump: ctx.bumps.treasury_token_account,
            bump: ctx.bumps.vesting_account,
        };
        Ok(())
    }

    pub fn create_employee_account(
        ctx: Context<CreateEmployeeAccount>,
        start_time: i64,
        end_time: i64,
        total_amount: i64,
        cliff_time: i64
    ) -> Result<()> {
        *ctx.accounts.employee_account = EmployeeAccount {
            beneficiary: ctx.accounts.beneficiary.key(),
            start_time,
            end_time,
            total_amount,
            total_withdrawn: 0,
            cliff_time,
            vesting_account: ctx.accounts.vesting_account.key(),
            bump: ctx.bumps.employee_account,
        };
        Ok(())
    }

    pub fn claim_tokens(ctx: Context<ClaimTokens>, _company_name: u64) -> Result<()> {
        // 获取员工账户
        let employee_account = &mut ctx.accounts.employee_account;
        let now = Clock::get()?.unix_timestamp;
        // 查看是否到了可领取时间
        if now < employee_account.cliff_time {
            return Err(ErrorCode::ClaimNotAvailableYet.into());
        }
        // 计算当前时间与开始解锁时间差值
        let time_since_start = now.saturating_sub(employee_account.start_time);
        // 计算全部解锁时间和开始解锁时间差值
        let total_vesting_time = employee_account.end_time.saturating_sub(employee_account.start_time);
        // 计算当前时间已解锁的代币
        let vested_amount = if now >= employee_account.end_time {
            employee_account.total_amount
        } else {
            (employee_account.total_amount * time_since_start) / total_vesting_time
        };
        // 计算当前时间可领取的代币
        let claimable_amount = vested_amount.saturating_sub(employee_account.total_withdrawn);
        // 没代币可领就报错
        if claimable_amount == 0 {
            return Err(ErrorCode::NothingToClaim.into());
        }
        // 转账需要的账户
        let transfer_cpi_account = TransferChecked {
            from: ctx.accounts.treasury_token_account.to_account_info(),
            mint: ctx.accounts.mint.to_account_info(),
            to: ctx.accounts.employee_token_account.to_account_info(),
            authority: ctx.accounts.treasury_token_account.to_account_info(),
        };
        // 代币程序地址
        let cpi_program = ctx.accounts.token_program.to_account_info();
        // 合约金库种子
        let signer_seeds: &[&[&[u8]]] = &[
            &[
                b"vesting_treasury",
                ctx.accounts.vesting_account.company_name.as_ref(),
                &[ctx.accounts.vesting_account.treasury_bump],
            ],
        ];
        // 构造 CPI 调用上下文（合约调用另一个合约）
        let cpi_context = CpiContext::new(cpi_program, transfer_cpi_account).with_signer(signer_seeds);
        // 获取代币精度（如 USDT 是 6 位，SOL 是 9 位）
        let decimals = ctx.accounts.mint.decimals;
        // 执行安全转账（带精度校验，防止转错数量）
        token_interface::transfer_checked(cpi_context, claimable_amount as u64, decimals)?;
        // 更新已转账的代币数量
        employee_account.total_withdrawn += claimable_amount;
        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(company_name: String)]
pub struct ClaimTokens<'info> {
    #[account(mut)]
    pub beneficiary: Signer<'info>,
    #[account(
        mut,
        seeds = [b"employee_vesting", beneficiary.key().as_ref(), vesting_account.key().as_ref()],
        bump = employee_account.bump,
        has_one = beneficiary,
        has_one = vesting_account,
    )]
    pub employee_account: Account<'info, EmployeeAccount>,
    #[account(
        mut,
        seeds = [company_name.as_ref()],
        bump = vesting_account.bump,
        has_one = mint,
        has_one = treasury_token_account,
    )]
    pub vesting_account: Account<'info, VestingAccount>,
    pub mint: InterfaceAccount<'info, Mint>,
    #[account(mut)]
    pub treasury_token_account: InterfaceAccount<'info, TokenAccount>,
    #[account(
        init_if_needed,
        payer = beneficiary,
        associated_token::mint = mint,
        associated_token::authority = beneficiary,
        associated_token::token_program = token_program
    )]
    pub employee_token_account: InterfaceAccount<'info, TokenAccount>,
    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,

}

#[derive(Accounts)]
#[instruction(company_name: String)]
pub struct CreateVestingAccount<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
    #[account(
        init,
        payer = signer,
        space = 8 + VestingAccount::INIT_SPACE,
        seeds = [company_name.as_ref()],
        bump,
    )]
    pub vesting_account: Account<'info,VestingAccount>,
    pub mint: InterfaceAccount<'info, Mint>,
    #[account(
        init,
        token::mint = mint,
        token::authority = treasury_token_account,
        payer = signer,
        seeds = [b"vesting_treasury", company_name.as_bytes()],
        bump
    )]
    pub treasury_token_account: InterfaceAccount<'info, TokenAccount>,

    pub system_program: Program<'info, System>,
    pub token_program: Interface<'info, TokenInterface>,
}

#[account]
#[derive(InitSpace, Debug)]
// 这是一个代币的锁仓账户，
pub struct VestingAccount {
    pub owner: Pubkey,  // 谁创建的锁仓账户
    pub mint: Pubkey,   // 代币信息
    pub treasury_token_account: Pubkey, //代币账户仓库
    #[max_len(50)]
    pub company_name: String,   //公司名称
    pub treasury_bump: u8, // 代币账户的pda种子 bump
    pub bump: u8,   //本账户自身的pda bump
}

#[derive(Accounts)]
pub struct CreateEmployeeAccount<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    pub beneficiary: SystemAccount<'info>,
    #[account(has_one = owner)]
    pub vesting_account: Account<'info,VestingAccount>,
    pub mint: InterfaceAccount<'info, Mint>,
    #[account(
        init,
        space = 8 + EmployeeAccount::INIT_SPACE,
        payer = owner,
        seeds = [b"employee_vesting", beneficiary.key().as_ref(), vesting_account.key().as_ref()],
        bump
    )]
    pub employee_account: Account<'info, EmployeeAccount>,
    pub system_program: Program<'info, System>,
}

#[account]
#[derive(InitSpace, Debug)]
pub struct EmployeeAccount {
    pub beneficiary: Pubkey,    //员工钱包地址
    pub start_time: i64,    // 锁仓开始时间
    pub end_time: i64,      // 锁仓结束时间
    pub total_amount: i64,  //一共给员工多少币
    pub total_withdrawn: i64,   //员工拿走了多少
    pub cliff_time: i64,  //锁仓悬崖时间（未满时间不能领取）
    pub vesting_account: Pubkey, //这个员工属于那个公司
    pub bump: u8
}

#[error_code]
pub enum ErrorCode {
    #[msg("Claiming is not available yet.")]
    ClaimNotAvailableYet,
    #[msg("There is nothing to claim.")]
    NothingToClaim,
}
