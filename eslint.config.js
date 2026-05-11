import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import prettierConfig from "eslint-config-prettier";
import globals from "globals";

export default tseslint.config(
  {
    ignores: [
      "dist/**",
      "dist-ssr/**",
      "node_modules/**",
      "src-tauri/target/**",
      "src-tauri/gen/**",
      "test-results/**",
      "playwright-report/**",
      "coverage/**",
    ],
  },

  // Base JS + TS rules for all source-like files
  {
    files: ["**/*.{js,mjs,cjs,ts,tsx}"],
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: "module",
      globals: {
        ...globals.browser,
        ...globals.node,
      },
    },
    rules: {
      // Allow _-prefixed unused args/vars to mirror common TS conventions
      "@typescript-eslint/no-unused-vars": [
        "error",
        {
          argsIgnorePattern: "^_",
          varsIgnorePattern: "^_",
          caughtErrorsIgnorePattern: "^_",
        },
      ],
      // Downgrade since the codebase crosses the Tauri boundary where `any` is sometimes unavoidable
      "@typescript-eslint/no-explicit-any": "warn",
    },
  },

  // React frontend source
  {
    files: ["src/**/*.{ts,tsx}"],
    plugins: {
      "react-hooks": reactHooks,
      "react-refresh": reactRefresh,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "react-refresh/only-export-components": ["warn", { allowConstantExport: true }],
    },
  },

  // Node-only scripts and config files
  {
    files: ["scripts/**/*.{js,mjs,cjs}", "*.config.{js,cjs,mjs,ts}", "eslint.config.js"],
    languageOptions: {
      globals: globals.node,
    },
  },

  // Keep Prettier last so it disables any stylistic conflicts
  prettierConfig,
);
