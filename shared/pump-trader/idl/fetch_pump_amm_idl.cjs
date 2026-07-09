// One-shot: fetch the on-chain Anchor IDL for the PumpSwap (pump_amm) program
// and write it to pump_amm.json next to this script.
//
// Run with @solana/web3.js available on NODE_PATH, e.g. from the scratch dir:
//   NODE_PATH=/tmp/idlfetch/node_modules node fetch_pump_amm_idl.cjs
const fs = require("fs");
const path = require("path");
const zlib = require("zlib");
const { PublicKey } = require("@solana/web3.js");

const PUMP_AMM = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";

function readRpcUrl() {
  const envPath = path.resolve(__dirname, "..", "..", ".env");
  const txt = fs.readFileSync(envPath, "utf8");
  const m = txt.match(/^HELIUS_RPC_URL=(.+)$/m);
  if (!m) throw new Error("HELIUS_RPC_URL not found in .env");
  return m[1].trim();
}

async function main() {
  const programId = new PublicKey(PUMP_AMM);
  // Anchor IDL account address derivation.
  const base = PublicKey.findProgramAddressSync([], programId)[0];
  const idlAddr = await PublicKey.createWithSeed(base, "anchor:idl", programId);
  console.error("program   :", programId.toBase58());
  console.error("idl base  :", base.toBase58());
  console.error("idl addr  :", idlAddr.toBase58());

  const rpc = readRpcUrl();
  const resp = await fetch(rpc, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      method: "getAccountInfo",
      params: [idlAddr.toBase58(), { encoding: "base64" }],
    }),
  });
  const json = await resp.json();
  if (json.error) throw new Error("RPC error: " + JSON.stringify(json.error));
  if (!json.result || !json.result.value) {
    throw new Error("IDL account not found (program may not publish an on-chain IDL)");
  }
  const data = Buffer.from(json.result.value.data[0], "base64");
  // IdlAccount: [0..8] discriminator, [8..40] authority, [40..44] u32 len, then zlib data.
  const len = data.readUInt32LE(40);
  const compressed = data.subarray(44, 44 + len);
  const idlJson = JSON.parse(zlib.inflateSync(compressed).toString("utf8"));

  const outPath = path.resolve(__dirname, "pump_amm.json");
  fs.writeFileSync(outPath, JSON.stringify(idlJson, null, 2));
  console.error("wrote     :", outPath, `(${len} compressed bytes)`);

  const names = (idlJson.instructions || []).map((i) => i.name);
  console.error("ixs       :", names.join(", "));
}

main().catch((e) => {
  console.error("FAILED:", e.message);
  process.exit(1);
});
