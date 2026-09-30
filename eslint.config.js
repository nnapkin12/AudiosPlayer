import js from "@eslint/js";
import globals from "globals";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import jsxA11y from "eslint-plugin-jsx-a11y";

export default tseslint.config(
  {
    ignores: ["dist/**", "node_modules/**", "src-tauri/**", "coverage/**"],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  jsxA11y.flatConfigs.recommended,
  {
    files: ["src/**/*.{ts,tsx}"],
    languageOptions: {
      ecmaVersion: 2022,
      globals: globals.browser,
    },
    plugins: {
      "react-hooks": reactHooks,
      "react-refresh": reactRefresh,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "react-refresh/only-export-components": "off",
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_", caughtErrors: "none" },
      ],
      // Tauri command errors arrive as strings; `void promise` is the accepted
      // fire-and-forget idiom in this codebase.
      "@typescript-eslint/no-floating-promises": "off",
      // Icon-only buttons carry `title`; a11y labelling is tracked in Phase 4.
      "jsx-a11y/no-autofocus": "off",
      // Labels here wrap a <span> block of text plus the control.
      "jsx-a11y/label-has-associated-control": ["error", { assert: "either", depth: 4 }],
    },
  },
  {
    files: ["*.config.{js,ts}", "postcss.config.js", "tailwind.config.js"],
    languageOptions: {
      globals: globals.node,
    },
  },
);
