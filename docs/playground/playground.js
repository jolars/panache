import init, { format_qmd_with_options } from "./pkg/panache_wasm.js";

const SAMPLE = "---\ntitle: Demo\n---\n\nThis is a very long line that should be wrapped.\n\n```{r}\nprint(1)\n```\n\n$$\nA &= B \\\\\nC &= D\n$$\n";

const $ = (id) => document.getElementById(id);
const statusEl = $("status");

function setStatus(text, isError = false) {
  statusEl.textContent = text;
  statusEl.classList.toggle("error", isError);
}

function readInt(id, fallback) {
  const value = parseInt($(id).value || String(fallback), 10);
  return Number.isNaN(value) ? fallback : value;
}

function currentOptions() {
  return {
    flavor: $("flavor").value,
    lw: readInt("lw", 80),
    wrap: $("wrap").value,
    lineEnding: $("line-ending").value,
    mathDelimiterStyle: $("math-delimiter-style").value,
    math: $("math").value,
    mathIndent: readInt("math-indent", 2),
    tabStops: $("tab-stops").value,
    tabWidth: readInt("tab-width", 4),
    extensions: $("extensions").value,
  };
}

function validateOptions(options) {
  if (options.lw < 10 || options.lw > 200) {
    return "Line width must be between 10 and 200.";
  }
  if (options.tabWidth < 1 || options.tabWidth > 16) {
    return "Tab width must be between 1 and 16.";
  }
  if (options.mathIndent < 0 || options.mathIndent > 20) {
    return "Math indent must be between 0 and 20.";
  }
  return null;
}

function updateQueryString(options) {
  const params = new URLSearchParams({
    flavor: options.flavor,
    lw: String(options.lw),
    wrap: options.wrap,
    lineEnding: options.lineEnding,
    mathDelimiterStyle: options.mathDelimiterStyle,
    math: options.math,
    mathIndent: String(options.mathIndent),
    tabStops: options.tabStops,
    tabWidth: String(options.tabWidth),
  });
  if (options.extensions.trim()) {
    params.set("extensions", options.extensions);
  }
  history.replaceState(null, "", "?" + params.toString());
}

function applyQueryString() {
  const query = new URLSearchParams(location.search);
  const controls = {
    flavor: "flavor",
    lw: "lw",
    wrap: "wrap",
    lineEnding: "line-ending",
    mathDelimiterStyle: "math-delimiter-style",
    math: "math",
    mathIndent: "math-indent",
    tabStops: "tab-stops",
    tabWidth: "tab-width",
    extensions: "extensions",
  };

  for (const [key, id] of Object.entries(controls)) {
    const value = query.get(key);
    if (value !== null) {
      $(id).value = value;
    }
  }

}

function applyFormat() {
  const input = $("input").value;
  const options = currentOptions();
  const validationError = validateOptions(options);
  updateQueryString(options);

  if (validationError) {
    $("output").value = "";
    setStatus(validationError, true);
    return;
  }

  try {
    $("output").value = format_qmd_with_options(
      input,
      options.lw,
      options.flavor,
      options.wrap,
      undefined,
      options.lineEnding,
      options.mathDelimiterStyle,
      options.math,
      options.tabStops,
      options.tabWidth,
      options.mathIndent,
      undefined,
      options.extensions,
    );
    setStatus("Formatted successfully.");
  } catch (error) {
    $("output").value = "";
    setStatus("Formatting failed: " + (error?.message || String(error)), true);
  }
}

function resetSample() {
  $("input").value = SAMPLE;
  applyFormat();
}

async function main() {
  await init();

  applyQueryString();

  $("input").addEventListener("input", applyFormat);
  [
    "flavor",
    "lw",
    "wrap",
    "line-ending",
    "math-delimiter-style",
    "math",
    "math-indent",
    "tab-stops",
    "tab-width",
    "extensions",
  ].forEach((id) => {
    $(id).addEventListener("input", applyFormat);
  });
  $("reset-input").addEventListener("click", resetSample);
  $("copy-output").addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText($("output").value);
      setStatus("Output copied to clipboard.");
    } catch {
      setStatus("Could not copy output in this browser context.", true);
    }
  });

  resetSample();
}

main().catch((error) => {
  setStatus("Failed to initialize playground: " + (error?.message || String(error)), true);
});
