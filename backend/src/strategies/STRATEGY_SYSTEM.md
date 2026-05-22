# Strategy System Architecture

## Overview
The strategy system provides a framework for defining and executing trading strategies based on token analysis and price movements.

## Components

### 1. Database Tables

#### `strategy_TPSL_rules`
Stores TPSL (Take Profit Stop Loss) strategy rules with the following parameters:
- `id`: Unique identifier for the rule
- `rule_name`: Human-readable name for the rule
- `p_initial_buy_sol`: Initial buy amount in SOL (token creation filter)
- `p_cu_limit`: Compute unit limit constraint (optional)
- `p_cu_price`: Compute unit price constraint (optional)
- `p_ix_labels`: Instruction labels filter (JSON array, optional)
- `buy_amount`: SOL amount to allocate per buy trade
- `take_profit`: Take profit percentage (e.g., 50 for 50% gain)
- `stop_loss`: Stop loss percentage (e.g., 20 for 20% loss)
- `is_active`: Whether this rule is currently active
- `created_at`, `updated_at`: Timestamps

#### `positions`
Tracks open and closed trading positions with the following fields:
- `id`: Unique position identifier
- `mint`: Token mint address (SPL token address)
- `wallet`: Wallet address that owns the position
- `entry_price`: SOL per token at entry
- `exit_price`: SOL per token at exit (NULL for open positions)
- `entry_tx`: Buy transaction signature
- `exit_tx`: Sell transaction signature (NULL for open positions)
- `status`: "Holding" (open position) or "End" (closed position)
- `strategy`: Strategy type (currently "TPSL", extensible for future strategies)
- `rule_id`: References the rule that triggered this position
- `entry_amount`: Number of tokens bought
- `exit_amount`: Number of tokens sold (NULL for open positions)
- `created_at`, `updated_at`: Timestamps

### 2. Models (`backend/src/models/`)

#### `position.rs`
- `Position` struct: Represents a trading position
  - Methods: `new()`, `close()`, `pnl_percentage()`
- `PositionStatus` enum: "Holding" or "End"

#### `strategy_tpsl_rule.rs`
- `StrategyTPSLRule` struct: Represents a TPSL strategy rule
  - Methods: `new()`

### 3. Strategy Handlers (`backend/src/strategies/`)

#### `tpsl.rs`
`TPSLStrategyHandler` - Main handler for TPSL strategy

**Methods:**
- `new(rules: Vec<StrategyTPSLRule>)`: Initialize handler with rules
- `check_buy_entry(token: &Token) -> Option<Uuid>`: 
  - Analyzes a new token to see if it matches any buy entry rule
  - Returns the rule_id if a match is found, None otherwise
  - Checks constraints: p_initial_buy_sol, p_cu_limit, p_cu_price, p_ix_labels
  
- `check_exit(position: &Position, current_price: f64, rule: &StrategyTPSLRule) -> Option<ExitReason>`:
  - Checks if an open position should exit based on current price
  - Compares against take_profit and stop_loss thresholds
  - Returns `ExitReason::TakeProfit` or `ExitReason::StopLoss` if conditions met
  
- `get_rule(rule_id: Uuid) -> Option<&StrategyTPSLRule>`: Retrieve a specific rule

## Workflow

### 1. Token Creation Event
When a new token is created on Pump.fun:
```
Token created → check_buy_entry() → Rule matched?
                ↓
              YES → Create Position with status="Holding"
                ↓
              NO → Skip token
```

### 2. Trade Event
When a trade occurs on an existing token:
```
Trade event received → Check all open Positions for this mint
                    ↓
                 For each Position → check_exit() with current price
                    ↓
                 Exit condition met? 
                    ↓
              YES → Close Position, set status="End", record exit_tx and exit_price
                ↓
              NO → Continue monitoring
```

## Future Extensions

The system is designed to support multiple strategies:
- `strategy_MACD_rules` table and `MAACDStrategyHandler`
- `strategy_RSI_rules` table and `RSIStrategyHandler`
- `strategy_MOMENTUM_rules` table and `MOMENTUMStrategyHandler`
- etc.

Each strategy would:
1. Have its own rules table in PostgreSQL
2. Have its own handler module in `backend/src/strategies/`
3. Implement the `StrategyHandler` trait (base trait for all strategies)

## Rule Parameters Explanation

### Buy Entry Parameters
- **p_initial_buy_sol**: The initial amount the creator put in (SOL). Used to filter tokens by creation style.
- **p_cu_limit**: Compute unit limit. Can indicate deliberate optimization or sophisticated creation.
- **p_cu_price**: Compute unit price. Relates to transaction speed/priority during creation.
- **p_ix_labels**: Instruction labels from the creation transaction. Can identify specific patterns or pre-configured setups.

### Exit Parameters
- **buy_amount**: How much SOL to spend on each buy (separate from entry filter)
- **take_profit**: Exit all holdings when price increases by this percentage
- **stop_loss**: Exit all holdings when price decreases by this percentage (protection)

## Database Constraints

- Foreign key: `positions.rule_id` → `strategy_TPSL_rules.id`
- Unique constraints: 
  - `entry_tx` (one position per buy transaction)
  - `exit_tx` (one position per exit transaction)
- Status check: Only "Holding" or "End" allowed
- Strategy check: Flexible to support multiple strategies

## Integration Points

To fully integrate the strategy system into the trading pipeline:

1. **Token Ingestion** (`backend/src/ingest/`):
   - On new token creation: Call `TPSLStrategyHandler.check_buy_entry()`
   - Create Position in DB if rule matches

2. **Trade Ingestion** (`backend/src/ingest/`):
   - On trade event: Query open Positions for that mint
   - Call `TPSLStrategyHandler.check_exit()` for each position
   - Update Position status and exit fields if exit condition met

3. **API Endpoints** (`backend/src/api/`):
   - CRUD operations for strategy rules
   - Query positions by various filters (status, strategy, wallet, etc.)
   - Performance metrics per rule/strategy

4. **State Management** (`backend/src/state/`):
   - Cache active rules in memory for fast lookups
   - Update cache when rules are created/modified/deleted
