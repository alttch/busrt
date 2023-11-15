import { defineConfig } from "vite";

export default defineConfig({
  build: {
    rollupOptions: {
      external: ["node:net"]
    },
    lib: {
      entry: "./src/busrt.ts",
      name: "busrt"
    }
  }
});
