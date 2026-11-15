// A fake Tauri backend for the GUI demo (see demo/record.mjs).
// It answers the Tauri commands with fictional data, so the real frontend runs in a normal browser.
// The data matches the fictional pages and CV in ../../demo.

import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import type { JobRow, Settings } from "../src/lib/types";

const NORDLICHT = "http://127.0.0.1:8765/nordlicht-careers.html";
const RUHRJOBS = "http://127.0.0.1:8765/jobboard.html";
const CV_PATH = "/Users/alex/Documents/demo_cv.txt";

const settings: Settings = {
  config: {
    active: "claude",
    providers: { claude: { type: "anthropic", model: "claude-haiku-5-5" } },
    output_language: "English",
    extra_instructions: "",
  },
  vars: { port: 8765 },
  sources: [
    { name: "Nordlicht Robotics (careers page)", url: NORDLICHT, enabled: true },
    { name: "RuhrJobs (job board)", url: RUHRJOBS, enabled: true },
  ],
  ignore: [{ company: "Kessler Logistik", note: "applied 2026-11-01" }],
  api_keys: { ANTHROPIC_API_KEY: "sk-ant-demo" },
  env_keys: [],
};

const rows: JobRow[] = [
  {
    match_score: 90,
    title: "Embedded Rust Developer (m/w/d)",
    company: "Nordlicht Robotics",
    location: "Bochum · Hybrid (2 days remote)",
    description:
      "You develop firmware for our drive control on ARM Cortex-M in Rust (embassy). Experience with CAN bus and real-time systems is a plus. Graduates are welcome.",
    match_reason:
      "Strong match: firmware in C and Rust, CAN bus and FreeRTOS from the working student job, and a thesis on Rust on Cortex-M. Bochum is near Witten.",
    url: "http://127.0.0.1:8765/jobs/embedded-rust-developer",
    source: NORDLICHT,
  },
  {
    match_score: 85,
    title: "Junior Software Developer C/C++ (m/w/d)",
    company: "Ruhrbyte Software GmbH",
    location: "Witten · On site",
    description: "Control software for measuring devices in C and C++ on Linux. Career starters welcome.",
    match_reason: "Entry-level role in C/C++ on Linux, in the home town of the candidate.",
    url: "http://127.0.0.1:8765/job/1002",
    source: RUHRJOBS,
  },
  {
    match_score: 75,
    title: "Firmware Engineer C (m/w/d)",
    company: "Ruhrtal Sensorik GmbH",
    location: "Hagen (15 km)",
    description: "Firmware for gas sensors in C on STM32, communication over I2C and SPI.",
    match_reason: "Embedded C with I2C and SPI fits the skills. The location is near Witten.",
    url: "http://127.0.0.1:8765/job/1001",
    source: RUHRJOBS,
  },
  {
    match_score: 60,
    title: "Backend Developer Python (m/w/d)",
    company: "Nordlicht Robotics",
    location: "Bochum · On site",
    description: "Fleet control with Python, FastAPI and PostgreSQL. At least 3 years of backend experience.",
    match_reason: "Python is a skill, but the job asks for 3 years of backend experience.",
    url: "http://127.0.0.1:8765/jobs/backend-python",
    source: NORDLICHT,
  },
  {
    match_score: 40,
    title: "Working Student Data Engineering (m/w/d)",
    company: "Fernweh Media GmbH",
    location: "Essen (22 km) · Remote possible",
    description: "Data pipelines with Python and SQL, 15 to 20 hours per week.",
    match_reason: "A part-time student job in data engineering, not a full-time embedded role.",
    url: "http://127.0.0.1:8765/job/1004",
    source: RUHRJOBS,
  },
  {
    match_score: 30,
    title: "Senior Cloud Architect AWS (m/w/d)",
    company: "Nordlicht Robotics",
    location: "Munich · On site",
    description: "Cloud architecture on AWS with Kubernetes and Terraform. At least 8 years of experience.",
    match_reason: "Needs 8 years of experience and cloud architecture. Far from Witten.",
    url: "http://127.0.0.1:8765/jobs/cloud-architect",
    source: NORDLICHT,
  },
];

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
const log = (msg: string, level: "info" | "warn" = "info") => emit("log", { level, msg });

let stopped = false;

async function fakeRun() {
  stopped = false;
  const sources = settings.sources;
  await log("CV: demo_cv.txt (107 words)");
  await wait(700);
  for (const [i, s] of sources.entries()) {
    if (stopped) break;
    await emit("progress", { step: "source", index: i + 1, total: sources.length, name: s.name });
    await log(`Download ${s.url}`);
    await wait(900);
    await log("Page has 1 part, asking claude-haiku-5-5 ...");
    await wait(1300);
    const jobs = rows.filter((r) => r.source === s.url).length;
    await log(`${s.name}: ${jobs} jobs`);
    await emit("progress", { step: "source_done", name: s.name, jobs, error: null });
  }
  if (sources.length && !stopped) await log("Ignore list: 1 job removed (Kessler Logistik)");
  await wait(500);
  await emit("run-done", {
    rows: stopped ? [] : rows,
    usage: [{ name: "extract", input: 9120, output: 1630, calls: 2 }],
    failed: 0,
    error: null,
    stopped,
  });
}

mockWindows("main");
mockIPC(
  async (cmd, args) => {
    switch (cmd) {
      case "app_info":
        return { data_dir: "/Users/alex/Library/Application Support/job-radar" };
      case "load_settings":
        return structuredClone(settings);
      case "save_settings":
        return null;
      case "cv_info":
        await wait(300);
        return { words: 107 };
      case "start_run":
        fakeRun();
        return null;
      case "stop_run":
        stopped = true;
        return null;
      case "add_ignore_rule":
        settings.ignore.push((args as { rule: Settings["ignore"][number] }).rule);
        return settings.ignore;
      case "test_connection":
        return "OK: claude-haiku-5-5 answered.";
      case "plugin:dialog|open":
        return CV_PATH;
      default:
        return null;
    }
  },
  { shouldMockEvents: true },
);

// The recording script uses these to simulate a file drop from Finder.
const at = { x: 0, y: 0 };
(window as unknown as { demo: unknown }).demo = {
  dragEnter: () => emit("tauri://drag-enter", { paths: [CV_PATH], position: at }),
  drop: () => emit("tauri://drag-drop", { paths: [CV_PATH], position: at }),
};

// Start with no CV, so the demo shows the upload.
try {
  localStorage.clear();
} catch {
  /* not important */
}
