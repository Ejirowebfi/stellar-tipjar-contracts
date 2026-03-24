# Guide: Sending a Basic Tip

This walkthrough covers the full lifecycle of a standard tip: deploying the contract, initializing it, and sending a tip that a creator can then withdraw.

---

## Prerequisites

- Stellar CLI installed and configured for testnet
- A funded sender account and a creator account on testnet
- Contract deployed (see `scripts/deploy.sh` or `README.md`)

---

## Step 1 — Initialize the Contract

`init` must be called exactly once after deployment. It sets the token and the admin address.

```bash
stellar contract invoke \
  --network testnet \
  --id <CONTRACT_ID> \
  --source <DEPLOYER_SECRET_KEY> \
  -- init \
  --token <TOKEN_CONTRACT_ID> \
  --admin <ADMIN_ADDRESS>
```

After this call:
- The contract accepts tips in the specified token.
- Only `<ADMIN_ADDRESS>` can pause or unpause the contract.

---

## Step 2 — Approve the Token Transfer

The sender must have a sufficient token balance. The contract calls `token::transfer` on behalf of the sender, so the sender must authorize the invocation.

When invoking via the CLI with `--source`, authorization is handled automatically. In a client application, the sender signs the transaction.

---

## Step 3 — Send the Tip

```bash
stellar contract invoke \
  --network testnet \
  --id <CONTRACT_ID> \
  --source <SENDER_SECRET_KEY> \
  -- tip \
  --sender <SENDER_ADDRESS> \
  --creator <CREATOR_ADDRESS> \
  --amount 250
```

What happens on-chain:
1. `sender.require_auth()` is checked.
2. `250` tokens are transferred from `sender` → contract escrow.
3. `CreatorBalance(creator)` increases by `250`.
4. `CreatorTotal(creator)` increases by `250`.
5. Event `("tip", creator)` is emitted with data `(sender, 250)`.

---

## Step 4 — Verify Balances

Check the creator's withdrawable balance:

```bash
stellar contract invoke \
  --network testnet \
  --id <CONTRACT_ID> \
  -- get_withdrawable_balance \
  --creator <CREATOR_ADDRESS>
```

Check the cumulative total:

```bash
stellar contract invoke \
  --network testnet \
  --id <CONTRACT_ID> \
  -- get_total_tips \
  --creator <CREATOR_ADDRESS>
```

Both should return `250` at this point.

---

## Step 5 — Creator Withdraws

The creator calls `withdraw` to pull their escrowed balance to their own address.

```bash
stellar contract invoke \
  --network testnet \
  --id <CONTRACT_ID> \
  --source <CREATOR_SECRET_KEY> \
  -- withdraw \
  --creator <CREATOR_ADDRESS>
```

What happens on-chain:
1. `creator.require_auth()` is checked.
2. `250` tokens are transferred from contract escrow → `creator`.
3. `CreatorBalance(creator)` is reset to `0`.
4. Event `("withdraw", creator)` is emitted with data `250`.

After withdrawal, `get_withdrawable_balance` returns `0` and `get_total_tips` still returns `250`.

---

## Summary

```
[Sender] --tip(250)--> [Contract Escrow] --withdraw()--> [Creator]

CreatorBalance:  0 → 250 → 0
CreatorTotal:    0 → 250   (never resets)
```
