# Guide: Recurring Tips / Subscriptions

The TipJar contract does not have built-in subscription or recurring payment logic — there is no on-chain scheduler. Recurring tips are implemented at the **application layer** by periodically invoking `tip` or `tip_with_message` on a schedule.

---

## Pattern: Off-Chain Scheduler

A backend service (cron job, Lambda, etc.) holds the sender's signing key and submits a tip transaction on a fixed interval.

```
[Scheduler] → tip(sender, creator, amount) every N days
```

### Example: Monthly Tip via Cron

```bash
# crontab entry — runs on the 1st of every month at 09:00 UTC
0 9 1 * * /usr/local/bin/send-monthly-tip.sh
```

```bash
#!/usr/bin/env bash
# send-monthly-tip.sh

stellar contract invoke \
  --network mainnet \
  --id "$CONTRACT_ID" \
  --source "$SENDER_SECRET_KEY" \
  -- tip \
  --sender "$SENDER_ADDRESS" \
  --creator "$CREATOR_ADDRESS" \
  --amount 500
```

The sender must maintain a sufficient token balance for each scheduled invocation.

---

## Pattern: Subscription with Metadata Tagging

Use `tip_with_message` to tag each recurring tip with a period identifier, making it easy to audit the subscription history on-chain.

```bash
PERIOD=$(date +"%Y-%m")   # e.g. "2026-03"

stellar contract invoke \
  --network mainnet \
  --id "$CONTRACT_ID" \
  --source "$SENDER_SECRET_KEY" \
  -- tip_with_message \
  --sender "$SENDER_ADDRESS" \
  --creator "$CREATOR_ADDRESS" \
  --amount 500 \
  --message "Monthly subscription — $PERIOD" \
  --metadata "{\"type\": \"subscription\", \"period\": \"$PERIOD\"}"
```

The `tip_msg` event and the stored `TipWithMessage` record both carry the metadata, so the creator's message history doubles as a subscription ledger.

---

## Querying Subscription History

Retrieve all stored messages for a creator to reconstruct the subscription timeline:

```bash
stellar contract invoke \
  --network mainnet \
  --id "$CONTRACT_ID" \
  -- get_messages \
  --creator "$CREATOR_ADDRESS"
```

Filter by `metadata.type == "subscription"` in your application to isolate recurring payments from one-off tips.

---

## Cancellation

Because subscriptions are off-chain, cancellation means stopping the scheduler. No on-chain action is required. The creator's balance simply stops growing.

---

## Considerations

| Concern | Recommendation |
|---|---|
| Sender key security | Use a dedicated low-balance account; top it up periodically |
| Missed payments | Log each invocation; retry on failure before the next cycle |
| Pause awareness | Check contract pause state before submitting; handle errors gracefully |
| Token allowance | Ensure the sender's token balance covers at least one period's tip |

---

## Limitations

- No on-chain enforcement of subscription terms — the sender can stop at any time.
- No refund mechanism if a sender overpays or wants to cancel mid-period.
- Message storage grows unboundedly; consider archiving old messages off-chain for high-frequency subscriptions.
