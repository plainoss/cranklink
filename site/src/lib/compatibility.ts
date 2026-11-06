export interface CompatibilityTable {
  columns: string[];
  rows: string[][];
}

/** Read the two documented tables, failing the build if the README format changes. */
export function readCompatibility(markdown: string): {
  motors: CompatibilityTable;
  parts: CompatibilityTable;
} {
  function table(
    heading: string,
    expectedColumns: string[],
  ): CompatibilityTable {
    const start = markdown.indexOf(`### ${heading}\n`);
    if (start < 0)
      throw new Error(`README compatibility heading missing: ${heading}`);
    const section = markdown.slice(start).split(/\n#{1,3} /)[0];
    const lines = section.split("\n").filter((line) => line.startsWith("|"));
    const cells = (line: string) =>
      line
        .split("|")
        .slice(1, -1)
        .map((cell) => cell.trim().replaceAll("**", "").replaceAll("`", ""));
    const columns = cells(lines[0] || "");
    if (columns.join("|") !== expectedColumns.join("|")) {
      throw new Error(
        `README ${heading} columns changed; update the site compatibility renderer.`,
      );
    }
    if (!/^\|[\s:|\-]+\|$/.test(lines[1] || "")) {
      throw new Error(`README ${heading} table separator missing.`);
    }
    const rows = lines.slice(2).map(cells);
    if (
      !rows.length ||
      rows.some(
        (row) => row.length !== columns.length || row.some((cell) => !cell),
      )
    ) {
      throw new Error(
        `README ${heading} compatibility rows are empty or malformed.`,
      );
    }
    return { columns, rows };
  }

  return {
    motors: table("Motors", [
      "Motor",
      "Motor firmware",
      "What you can change",
      "Status",
      "Firmware recovery",
    ]),
    parts: table("Other parts", ["Part", "What you can change", "Models"]),
  };
}
