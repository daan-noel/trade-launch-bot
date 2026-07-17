{
    "take_profit": 100,
    "stop_loss": 30,
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
            "trail": [
                {
                    "operator": "<",
                    "value": 10
                }
            ]
        },
        "m_time_window": {
            "window_size_sec": 10,
            "gross_flow": [
                {
                    "operator": "=",
                    "value": 15
                }
            ],
            "net_flow": [
                {
                    "operator": "=",
                    "value": 5
                }
            ],
            "buy": [
                {
                    "operator": "=",
                    "value": 10
                }
            ],
            "sell": [
                {
                    "operator": "=",
                    "value": 5
                }
            ],
        }
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
            "trail": [
                {
                    "operator": "<",
                    "value": 10
                }
            ]
        },
        "m_time_window": {
            "window_size_sec": 10,
            "gross_flow": [
                {
                    "operator": "=",
                    "value": 15
                }
            ],
            "net_flow": [
                {
                    "operator": "=",
                    "value": 5
                }
            ],
            "buy": [
                {
                    "operator": "=",
                    "value": 10
                }
            ],
            "sell": [
                {
                    "operator": "=",
                    "value": 5
                }
            ],
        }
    }
}