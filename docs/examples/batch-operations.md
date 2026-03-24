# Guide: Batch Operations

The TipJar contract does not expose a native batch function, but multiple tips can be submitted in a single Stellar transaction using **operation bundling** at the transaction level, or sequentially in a script.

---

## Option A — Sequential Tips in a Script

The simplest approach: loop over a list of (creator, amount) pairs and invoke `tip` for each.

```bash
#!/usr/bin/env bash
# batch-tip.sh — send tips to multiple creators from one sender

CONTRACT_ID="<CONTRACT_ID>"
SENDER_KEY="<SENDER_SECRET_KEY>"
SENDER_ADDR="<SENDER_ADDRESS>"
NETWORK="testnet"

declare -A TIPS=(
  ["GCREATOR_A"]=100
  ["GCREATOR_B"]=200
  ["GCREATOR_C"]=50
)

for CREATOR in "${!TIPS[@]}"; do
  AMOUNT="${TIPS[$CREATOR]}"
  echo "Tipping $CREATOR with $AMOUNT..."
  stellar contract invoke \
    --network "$NETWORK" \
    --id "$CONTRACT_ID" \
    --source "$SENDER_KEY" \
    -- tip \
    --sender "$SENDER_ADDR" \
    --creator "$CREATOR" \
    --amount "$AMOUNT"
done
```

Each invocation is a separate transaction. The sender's token balance must cover the sum of all amounts.

---

## Option B — Tips with Messages in Bulk

Use `tip_with_message` in the same loop pattern when you want to attach per-creator notes:

```bash
stellar contract invoke \
  --network testnet \
  --id "$CONTRACT_ID" \
  --source "$SENDER_KEY" \
  -- tip_with_message \
  --sender "$SENDER_ADDR" \
  --creator "$CREATOR" \
  --amount 150 \
  --message "Thanks for the tutorial series!" \
  --metadata '{"campaign": "launch-week"}'
```

---

## Option C — Batch Withdrawals (Multiple Creators)

If you manage multiple creator accounts and want to sweep all balances, iterate over each creator:

```bash
#!/usr/bin/env bash
# batch-withdraw.sh

CONTRACT_ID="<CONTRACT_ID>"
NETWORK="testnet"

CREATORS=(
  "GCREATOR_A:<SECRET_A>"
  "GCREATOR_B:<SECRET_B>"
)

for ENTRY in "${CREATORS[@]}"; do
  ADDR="${ENTRY%%:*}"
  KEY="${ENTRY##*:}"
  echo "Withdrawing for $ADDR..."
  stellar contract invoke \
    --network "$NETWORK" \
    --id "$CONTRACT_ID" \
    --source "$KEY" \
    -- withdraw \
    --creator "$ADDR"
done
```

> Each creator must sign their own `withdraw` call — the contract enforces `creator.require_auth()`. You cannot withdraw on behalf of another creator.

---

## Checking Balances Before Withdrawing

To avoid `NothingToWithdraw` errors, query the balance first:

```bash
BALANCE=$(stellar contract invoke \
  --network testnet \
  --id "$CONTRACT_ID" \
  -- get_withdrawable_balance \
  --creator "$ADDR")

if [ "$BALANCE" -gt 0 ]; then
  stellar contract invoke --source "$KEY" -- withdraw --creator "$ADDR"
fi
```

---

## Notes

- There is no atomic multi-tip function. Each `tip` call is an independent transaction.
- Token approval is implicit — the sender authorizes each transaction individually.
- For high-volume use cases, consider batching at the application layer and monitoring the `tip` event stream to reconcile state.
