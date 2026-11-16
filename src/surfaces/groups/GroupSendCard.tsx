// =====================================================================
// citrate-core: the @Hermes send card (V6, owner decision 2026-11-16).
//
// Hermes's answer in a group is never posted on the @mention alone. The member's app shows this
// card with the exact text, how many members receive it, what Hermes read, and the result of the
// on-device private-information check (src-tauri/src/pii_scan.rs). The member posts it, posts the
// scanner's redaction, edits it, or does not post it. When the check finds something, the answer
// as written cannot be posted from here, and groupHermes.answerInGroup re-checks whatever the member
// chooses, so a finding blocks the send whatever this card shows.
// =====================================================================
import { useState } from "react";
import { NOT_READ, type SendDecision, type SendPreview } from "./groupHermes";

/** "Post to 6 members", or "Post to the group" when the roster was not read. */
export function postLabel(recipients: number | null): string {
  if (recipients === null) return "Post to the group";
  return `Post to ${recipients} ${recipients === 1 ? "member" : "members"}`;
}

const PLURALS: Record<string, string> = {
  "person's name": "names",
  "key, token or seed phrase": "keys, tokens or seed phrases",
  "bank account (IBAN)": "bank accounts (IBAN)",
};

/** "1 email address, 2 phone numbers". */
export function foundLine(found: SendPreview["scan"]["found"]): string {
  const plural = (l: string) => PLURALS[l] ?? (l.endsWith("ss") ? l + "es" : l + "s");
  return found.map((f) => `${f.count} ${f.count === 1 ? f.label : plural(f.label)}`).join(", ");
}

export function GroupSendCard({ preview, onDecide }: { preview: SendPreview; onDecide: (d: SendDecision) => void }) {
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(preview.text);
  const { scan } = preview;
  const edited = text.trim() !== preview.text.trim();
  return (
    <section role="dialog" aria-modal="false" aria-labelledby="grp-send-h" className="grp-alert" data-testid="group-send-card" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <h2 id="grp-send-h" style={{ fontSize: 14, fontWeight: 500, margin: 0 }}>
        Hermes drafted an answer. It is not posted yet.
      </h2>
      <p className="grp-faint" style={{ margin: 0 }}>
        {preview.recipients === null ? "Everyone in this group will see it." : `${preview.recipients} ${preview.recipients === 1 ? "member" : "members"} will see it.`} Check it before you post.
      </p>
      {editing ? (
        <>
          <label className="sr-only" htmlFor="grp-send-edit">Edit the answer before posting</label>
          <textarea id="grp-send-edit" className="input" rows={4} value={text} onChange={(e) => setText(e.target.value)} />
        </>
      ) : (
        <blockquote data-testid="group-send-preview" style={{ margin: 0, whiteSpace: "pre-wrap" }}>{preview.text}</blockquote>
      )}
      {preview.draft && <p className="grp-faint" style={{ margin: 0 }}>It includes a round draft for the owners and admins. A draft moves no SALT.</p>}
      {scan.passed ? (
        <p role="status" data-testid="group-send-scan" style={{ margin: 0, fontSize: 12 }}>
          Private-information check on this device: nothing found.
        </p>
      ) : (
        <div role="alert" data-testid="group-send-scan" style={{ fontSize: 12 }}>
          Private-information check on this device found {foundLine(scan.found)}. The answer cannot be posted as written. Post it with these removed, or edit it.
        </div>
      )}
      <details>
        <summary>What Hermes read ({preview.read.length})</summary>
        <ul style={{ margin: "4px 0", paddingLeft: 18 }}>
          {preview.read.map((r) => (
            <li key={r}>{r}</li>
          ))}
        </ul>
        <span className="grp-faint">{NOT_READ}</span>
      </details>
      <span className="grp-faint" style={{ fontSize: 11 }}>{scan.coverage}</span>
      <div className="grp-inline">
        {editing ? (
          <button className="btn btn-primary btn-sm" disabled={!text.trim()} onClick={() => onDecide({ post: true, text })}>
            {postLabel(preview.recipients)}
          </button>
        ) : scan.passed ? (
          <button className="btn btn-primary btn-sm" onClick={() => onDecide({ post: true, text: preview.text })}>
            {postLabel(preview.recipients)}
          </button>
        ) : (
          <button className="btn btn-primary btn-sm" onClick={() => onDecide({ post: true, text: scan.redacted })}>
            Post with these removed
          </button>
        )}
        {!editing && (
          <button className="btn btn-secondary btn-sm" onClick={() => setEditing(true)}>
            Edit
          </button>
        )}
        <button className="btn btn-ghost btn-sm" onClick={() => onDecide({ post: false })}>
          Don't post
        </button>
      </div>
      {editing && edited && <span className="grp-faint">Your edit is checked again on this device before it is posted.</span>}
    </section>
  );
}
