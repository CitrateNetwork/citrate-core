// =====================================================================
// citrate-core — daemon token meter (HUP-S10.3)
//
// No provider reports token usage to the app yet, so a daemon run's tokens are ESTIMATED from
// characters (about 4 characters per token), and every screen that shows them says "estimated".
// Each model round re-reads the system prompt, the tool list and the conversation so far, so a
// round costs `base + context`; the answer costs its own length. The meter calls `onExceed` once,
// as soon as the estimate passes the run's allowance; the runner then stops the run.
// =====================================================================
export const CHARS_PER_TOKEN = 4;

export class TokenMeter {
  private base = 0;
  private ctx = 0;
  private input = 0;
  private out = 0;
  private rounds = 0;
  private fired = false;

  constructor(
    private readonly allowance: number,
    private readonly onExceed: () => void,
  ) {}

  /** `baseChars`: system prompt + tool list; `taskChars`: the daemon's prompt. */
  begin(baseChars: number, taskChars: number): void {
    this.base = Math.max(0, baseChars);
    this.ctx = Math.max(0, taskChars);
  }

  /** One model request: it reads everything so far. */
  round(): void {
    this.rounds++;
    this.input += this.base + this.ctx;
    this.check();
  }

  /** Text added to the conversation (a tool call's arguments or result). */
  context(chars: number): void {
    this.ctx += Math.max(0, chars);
    this.check();
  }

  /** Text the model wrote. */
  output(chars: number): void {
    this.out += Math.max(0, chars);
    this.ctx += Math.max(0, chars);
    this.check();
  }

  tokens(): number {
    const input = this.rounds === 0 ? this.base + this.ctx - this.out : this.input;
    return Math.ceil((input + this.out) / CHARS_PER_TOKEN);
  }

  private check(): void {
    if (!this.fired && this.tokens() > this.allowance) {
      this.fired = true;
      this.onExceed();
    }
  }
}
