# TipJar Contract — Storage Schema

The contract uses two Soroban storage tiers: **Instance** (shared contract-level state) and **Persistent** (per-address state that survives ledger expiry with TTL extension).

---

## Instance Storage

Shared state for the entire contract. Stored under a single instance entry.

| Key | Type | Set By | Description |
|---|---|---|---|
| `DataKey::Token` | `Address` | `init` | The token contract address used for all tip transfers |
| `DataKey::Admin` | `Address` | `init` | The administrator address authorized to pause/unpause |
| `DataKey::Paused` | `bool` | `init`, `pause`, `unpause` | Emergency pause flag; `true` blocks tips and withdrawals |

These values are written once during `init` (except `Paused`, which toggles). They are never deleted.

---

## Persistent Storage

Per-creator state. Each entry is keyed by the creator's `Address`. These entries are subject to Soroban's ledger TTL and must be extended to remain accessible.

| Key | Type | Set By | Read By | Description |
|---|---|---|---|---|
| `DataKey::CreatorBalance(Address)` | `i128` | `tip`, `tip_with_message` | `get_withdrawable_balance`, `withdraw` | Current escrowed balance available for withdrawal. Reset to `0` after `withdraw`. |
| `DataKey::CreatorTotal(Address)` | `i128` | `tip`, `tip_with_message` | `get_total_tips` | Cumulative total of all tips ever received. Never decreases. |
| `DataKey::CreatorMessages(Address)` | `Vec<TipWithMessage>` | `tip_with_message` | `get_messages` | Ordered list of all `TipWithMessage` records for the creator. |

---

## `TipWithMessage` Schema

Stored as elements inside `CreatorMessages(Address)`.

| Field | Type | Description |
|---|---|---|
| `sender` | `Address` | Tipper's address |
| `creator` | `Address` | Recipient's address |
| `amount` | `i128` | Token amount |
| `message` | `String` | Attached note (max 280 chars) |
| `metadata` | `Map<String, String>` | Arbitrary key-value pairs |
| `timestamp` | `u64` | `env.ledger().timestamp()` at tip time |

---

## Balance vs. Total

These two values serve different purposes and should not be confused:

| | `CreatorBalance` | `CreatorTotal` |
|---|---|---|
| Increases on tip | ✅ | ✅ |
| Resets on withdraw | ✅ (→ 0) | ❌ |
| Use case | "How much can I withdraw?" | "How much have I ever earned?" |

---

## Storage Lifecycle

```
init()
  └─ instance: Token, Admin, Paused=false

tip() / tip_with_message()
  └─ persistent: CreatorBalance += amount
  └─ persistent: CreatorTotal  += amount
  └─ persistent: CreatorMessages.push(payload)  [tip_with_message only]

withdraw()
  └─ persistent: CreatorBalance = 0
```
