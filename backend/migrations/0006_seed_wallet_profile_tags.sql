-- Seed default tags for wallet profile classification
INSERT INTO wallet_profile_tags (name, color, comment) VALUES
    -- Performance labels
    ('Whale',           '#6366f1', 'Large position sizes, significant market impact'),
    ('Degen',           '#f43f5e', 'High-risk, high-frequency speculative trades'),
    ('Diamond Hands',   '#22d3ee', 'Holds through volatility, rarely exits early'),
    ('Paper Hands',     '#94a3b8', 'Exits positions quickly on any downturn'),
    ('Flipper',         '#f59e0b', 'Fast in-and-out, targets quick profits'),
    ('Sniper',          '#10b981', 'Enters very early, often at launch or listing'),
    ('Ape',             '#fb923c', 'Buys into hype with little due diligence'),
    ('Smart Money',     '#a78bfa', 'Consistently early on winners, likely informed'),
    ('Bot',             '#64748b', 'Automated trading patterns, likely scripted'),
    ('Bundler',         '#0ea5e9', 'Groups buys to obscure wallet identity'),

    -- Risk profile
    ('High Risk',       '#ef4444', 'Frequently trades micro-cap or low-liquidity tokens'),
    ('Low Risk',        '#84cc16', 'Prefers established tokens with deeper liquidity'),
    ('Leverage',        '#dc2626', 'Uses leveraged positions or perpetuals'),

    -- Behavior
    ('Copy Trader',     '#8b5cf6', 'Mirrors trades of known alpha wallets'),
    ('Insider',         '#f97316', 'Suspicious early buys before announcements'),
    ('Dev Wallet',      '#ec4899', 'Linked to a token team or deployer address'),
    ('MEV',             '#14b8a6', 'Sandwich attacks, frontrunning, or arbitrage activity'),
    ('Wash Trader',     '#78716c', 'Suspected artificial volume between related wallets'),
    ('Accumulator',     '#3b82f6', 'Builds positions gradually over time'),
    ('Distributor',     '#f87171', 'Consistently dumps into strength'),

    -- Community / social
    ('KOL',             '#fbbf24', 'Key Opinion Leader — influencer or CT personality'),
    ('Watchlist',       '#60a5fa', 'Under active monitoring for copy or alpha signals'),
    ('Blacklist',       '#1e293b', 'Known bad actor, scammer, or rug puller')

ON CONFLICT (name) DO NOTHING;
