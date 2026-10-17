export default {
  "*.rs": ({ filenames }) => `rustfmt --check ${filenames.join(" ")}`,
  "web-instrument/src/**/*.{ts,tsx}": () => "npm --prefix web-instrument run typecheck",
};
