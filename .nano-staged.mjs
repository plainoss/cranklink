export default {
  "*.rs": ({ filenames }) => `cargo fmt --check -- ${filenames.join(" ")}`,
  "web-instrument/src/**/*.{ts,tsx}": () => "npm --prefix web-instrument run typecheck",
};
