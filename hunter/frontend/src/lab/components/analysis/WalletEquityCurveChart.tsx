// The chart itself was promoted to `components/analytics/EquityCurveChart`
// (shared with the live Console History deck). This stays as the lab's named
// entry point so `WalletAnalyticsPanel`'s lazy import — and the chunk split that
// keeps `lightweight-charts` out of the main bundle — are unchanged.
export { EquityCurveChart as WalletEquityCurveChart } from 'components/analytics/EquityCurveChart';
