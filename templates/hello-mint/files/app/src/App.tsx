import { COLLECTION } from "./collection";
import type { Deployment } from "./config";
import { MintCard } from "./MintCard";

export function App({ deployment }: { deployment: Deployment }) {
  return (
    <main className="page">
      <header>
        <h1>{COLLECTION.name}</h1>
        <p className="network">
          {deployment.target === "fork" ? "Local fork of Citrate (chain 40204)" : "Citrate (chain 40204)"}
        </p>
      </header>
      <MintCard deployment={deployment} />
    </main>
  );
}

export function ConfigError({ message }: { message: string }) {
  return (
    <main className="page">
      <section className="card">
        <h2>Configuration problem</h2>
        <p>{message}</p>
        <p className="hint">Check .env.local against .env.example, then restart the dev server.</p>
      </section>
    </main>
  );
}
