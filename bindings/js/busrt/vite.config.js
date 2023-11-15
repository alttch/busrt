import { defineConfig } from "vite";

export default defineConfig({
  build: {
    lib: {
      entry: "./src/busrt.ts",
      name: "busrt"
    },
  }
});
