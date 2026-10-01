import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// base "./": asset paths are relative, so the built page also works when an IPFS gateway serves
// it under /ipfs/<cid>/ (HUP-S6.6), not only at a site root such as a Vercel domain.
export default defineConfig({
  base: "./",
  plugins: [react()],
});
