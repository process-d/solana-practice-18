# Solana Lending Protocol

A decentralized lending protocol built on Solana using the Anchor framework.

## Overview

This protocol enables users to:
- **Deposit** assets into lending pools
- **Borrow** against their collateral
- **Repay** loans with interest
- **Liquidate** undercollateralized positions

## Features

- [x] Multiple token support (SOL, USDC)
- [x] Pyth Oracle price feeds integration
- [x] Interest rate calculation
- [x] Liquidation mechanism
- [x] Bankrun testing framework

## Tech Stack

- **Blockchain**: Solana
- **Framework**: Anchor 0.30.1
- **Language**: Rust
- **Testing**: Bankrun + Mocha
- **Oracle**: Pyth Network

## Project Structure

```
lending/
├── Anchor.toml          # Anchor configuration
├── Cargo.toml          # Rust dependencies
├── programs/
│   └── lending/
│       └── src/
│           ├── lib.rs           # Program entry
│           ├── mod.rs           # Module exports
│           ├── state.rs         # Account state
│           ├── error.rs         # Error codes
│           ├── constants.rs     # Constants
│           └── instructions/    # Instructions
│               ├── admin.rs
│               ├── borrow.rs
│               ├── deposit.rs
│               ├── repay.rs
│               ├── withdraw.rs
│               └── liquidate.rs
└── tests/
    └── bankrun.spec.ts          # Integration tests
```

## Getting Started

### Prerequisites

- Rust 1.86+
- Solana CLI
- Anchor CLI 0.30.1
- Node.js 18+

### Build

```bash
# Install dependencies
yarn install

# Build the program
anchor build
```

### Test

```bash
anchor test
```

### Deploy

```bash
# Set your cluster
solana config set --url devnet

# Deploy
anchor deploy
```

## Program ID

```
Lending: 2NYVrByVMmi9BCTZox2QH2v92mfuD8Bg77gmg7Y2kSZ9
```

## Architecture

### Accounts

| Account | Description |
|---------|-------------|
| `Bank` | Lending pool configuration |
| `User` | User's deposit/borrow positions |
| `Treasury` | Protocol treasury |

### Instructions

| Instruction | Description |
|-------------|-------------|
| `initBank` | Initialize a lending pool |
| `initUser` | Initialize user account |
| `deposit` | Deposit assets |
| `borrow` | Borrow against collateral |
| `repay` | Repay loan |
| `withdraw` | Withdraw deposited assets |
| `liquidate` | Liquidate unhealthy positions |

## Oracle

Uses Pyth Network for price feeds:
- SOL/USD
- USDC/USD

## Security

- All amounts validated
- Liquidation threshold protection
- Interest rate calculations

## License

MIT

## Contributing

Contributions welcome! Please open an issue or PR.
