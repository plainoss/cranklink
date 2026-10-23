export default {
  "*.rs": ({ filenames }) => `cargo fmt --check -- ${filenames.join(" ")}`,
  "web-instrument/**/*.{ts,tsx}": () => "npm --prefix web-instrument run typecheck",
};
