# TipJar Contract — Events Reference

All events are emitted via `env.events().publish(topics, data)` and are queryable from Stellar's event streaming API.

---

## `tip`

Emitted when a plain tip is successfully transferred into escrow.

**Topics**

| Position | Value | Type |
|---|---|---|
| 0 | `"tip"` | `Symbol` |
| 1 | `creator` | `Address` |

**Data**

| Field | Type | Description |
|---|---|---|
| `sender` | `Address` | The address that sent the tip |
| `amount` | `i128` | Token amount transferred |

**Example (conceptual)**
```
topics: ["tip", "GCREATOR..."]
data:   ["GSENDER...", 250]
```

---

## `tip_msg`

Emitted when a tip is sent with an attached message via `tip_with_message`.

**Topics**

| Position | Value | Type |
|---|---|---|
| 0 | `"tip_msg"` | `Symbol` |
| 1 | `creator` | `Address` |

**Data**

| Field | Type | Description |
|---|---|---|
| `sender` | `Address` | The address that sent the tip |
| `amount` | `i128` | Token amount transferred |
| `message` | `String` | The attached message text |
| `metadata` | `Map<String, String>` | Arbitrary key-value metadata |

**Example (conceptual)**
```
topics: ["tip_msg", "GCREATOR..."]
data:   ["GSENDER...", 500, "Great content!", {"platform": "web"}]
```

---

## `withdraw`

Emitted when a creator successfully withdraws their escrowed balance.

**Topics**

| Position | Value | Type |
|---|---|---|
| 0 | `"withdraw"` | `Symbol` |
| 1 | `creator` | `Address` |

**Data**

| Field | Type | Description |
|---|---|---|
| `amount` | `i128` | Total amount withdrawn |

**Example (conceptual)**
```
topics: ["withdraw", "GCREATOR..."]
data:   400
```

---

## Querying Events

Use the Stellar CLI to stream or query events from a deployed contract:

```bash
stellar events \
  --network testnet \
  --contract-id <CONTRACT_ID> \
  --start-ledger <LEDGER>
```

Filter by topic to watch only tip events:

```bash
stellar events \
  --network testnet \
  --contract-id <CONTRACT_ID> \
  --topic1 tip
```
