// citrate-core: FL number words. SALT only (no USD, no projection), exact from wei (BigInt, no
// floats), and durations said as "about 3 h 10 min". A value that was not read is never passed
// here; the caller renders <Unknown/> instead.

const WEI = 10n ** 18n;

/** Wei as SALT with `digits` decimals, truncated (never rounded up): 10500000000000000000n → "10.50". */
export function saltFromWei(wei: bigint, digits = 2): string {
  const neg = wei < 0n;
  const v = neg ? -wei : wei;
  const whole = v / WEI;
  const frac = v % WEI;
  const scale = 10n ** BigInt(18 - digits);
  const f = digits > 0 ? (frac / scale).toString().padStart(digits, "0") : "";
  return (neg ? "-" : "") + whole.toLocaleString("en-US") + (digits > 0 ? "." + f : "");
}

/** "10.50 SALT" */
export function saltText(wei: bigint, digits = 2): string {
  return saltFromWei(wei, digits) + " SALT";
}

/** "about 3 h 10 min", "about 40 min", "under a minute". */
export function aboutDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 60) return "under a minute";
  const m = Math.round(seconds / 60);
  const h = Math.floor(m / 60);
  const rest = m % 60;
  if (h === 0) return `about ${m} min`;
  if (h >= 48) return `about ${Math.round(h / 24)} days`;
  return rest === 0 ? `about ${h} h` : `about ${h} h ${rest} min`;
}

/** "1,209,911" */
export function blockText(n: number | bigint): string {
  return n.toLocaleString("en-US");
}
