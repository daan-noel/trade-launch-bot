Let's discuss about solid strategy data structure, module structure, workflow, logics.
Don't think about current implementation status.
This is very big update/change - code, DB, folder structure, etc.
You must understand exactly, explicitly what I want.
Ask anything you think unexplicit.
You understand correctly and entirely first.


# 1.
A strategy will have a "Fingerprint" type - this fingerprint type is a group of params like cu_limit/cu_price, max_sol_cost/spendable_sol_in, first slot buy/sell, ix_orders, ..., etc.
```
{
    // priority fingerprint params
    "cu_limit": 300000,
    "cu_price": 3333333,
    // first buy fingerprint params
    "init_buy_amount: 3.5,
    "max_sol_cost": 3.5,
    "spendable_sol_in": 0,
    // first slot trade fingerprint params
    "first_slot_buy": 12,
    "first_slot_sell": 0,
    // bucket size
    "bucket_size_amount": 0.1,

    // instruction structure fingerprint params
    "ix_labels": [
        "Compute Budget: SetComputeUnitLimit",
        "Compute Budget: SetComputeUnitPrice",
        "Pump.Fun: Create_v2",
        "Associated Token: CreateIdempotent",
        "Pump.Fun: Buy",
        "System Program: Transfer"
    ]
}
```

This will be saved in DB as a table so that many other strategies can share.


# 2.
I'll make a specific track/detect metrics logic.
When a token is created and it's matched with a fingerprint type, the token will be "armed" status for all the rules using the same fingerprint.
And each time a new trade happens, calculate the status of token for each of the metrics.

These metrics type can be divide to 2 sides:
- static: when a new trade happens, these can be derived immediately without any more param values.
- dynamic: these values requires dynamic param values according to each strategy rules.
I'll explain about these more detailly later.

So by detecting/tracking whole metrics when a trade happens for a token,
all tokens(tracking tokens) will have their status/metric values group according to the trading status - maybe I can see this metric values in token trade history chart on UI to make my analyzing easily.

# 3. 
For entry/exit determindation,
entry/exit will have metrics group each other, so when a trade happened and current status of metrics are all matched with entry metrics group, enter those token.
Same for exit case as well.
The status metrics will be calculated constant values, but the entry/exit metrics will be group of  {value, operator} pairs.


# 4.
Now I'll explain about the metrics I'm thinking now - this is not completed yet and need to extend a lot.
## 1. Static metrics
These metrics can be calculated immediately when a trade happens.
```
snapshot metrics
{
    "time": 10 second,
    "liquidity": 20 sol,
    ...
}

price path metrics
{
    "stall": 30 second,
    "trail": 15 %,
    ....
}
```
## 2. Dynamic metrics
These metrics must be calculated with each rule's defined values - like "window_size_sec".
```
time window metrics
{
    "window_size_sec": 10 s,
    // alive amounts
    "gross_flow": 25 sol,
    "net_flow": 5 sol,
    "buy": 15 sol,
    "sell": 10 sol,
    ....
}
```

So when a trade happens, all the activated rules start to calculate dynamic metrics according to there defined values.
Right now, let's think about those 8 metrics - I'll add more metrics later, so exntensibility is very important.


# 5.
In current project, all the params have explicit operator logic.
For example, "min_age_sec=10" -> ">10", "max_age_sec=30" -> "<30", "init_buy_sol=3" -> "=3" with bucket-size, ...
When I input a number value, the project logic is already defined about the operator.
I want to make this dynamic - likely the filter inputs in DataTable - ">10, <=30".

Say again, for the metrics, I want to make dynamic operator logic.

# 6.
"strategy_rules" table -> "params" column is a JSONB-typed field.
This will hold both entry/exit metrics values as well as TP/SL:
```
{
    "take_profit": 100%,
    "stop_loss": 30%,
    "entry": {
        "m_snapshot": {
            "time": [
                {
                    "operator": ">",
                    "value": 10
                },
                {
                    "operator": "<",
                    "value": 30
                },
            ],
            "liquidity": [
                {
                    "operator": "=",
                    "value": 20
                }
            ],
        },
        "m_price_path": {
            "stall": [
                {
                    "operator": "<",
                    "value": 10
                }
            ],
            "trail": [...
            ]
        }
        "m_time_window": {...}
        ....
    },
    "exit": {
        "m_snapshot": {
            "time": [
                {
                    "operator": ">",
                    "value": 10
                },
                {
                    "operator": "<",
                    "value": 30
                },
            ],
            "liquidity": [
                {
                    "operator": "=",
                    "value": 20
                }
            ],
        },
        "m_price_path": {
            "stall": [
                {
                    "operator": "<",
                    "value": 10
                }
            ],
            "trail": [...
            ]
        }
        "m_time_window": {...}
        ....
    }
}
```


# 7.
For the metrics, I'll make them as seperated code files in metric/ folder:
- snapshot - time, liquidity
- price path - stall, trail
- time window - gross_flow, net_flow, buy, sell
- ... I will add other metrics later

Each file have the metric logics - for example, stall, trail have their own calculation logics.


# 8.
fingerprints, and metrics are seperated with strategy itself.
strategy rules are just combination of different metrics values and a fingerprint type.
(strategy rule = Fingerprint + Metrics values)


Build concrete and solid backend first, frontend will be needed to update a lot, so think about it later.
Keep - You ask, I answer - until you are confident you understand all correctly.