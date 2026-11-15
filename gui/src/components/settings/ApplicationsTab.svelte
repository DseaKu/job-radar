<script lang="ts">
  // Where and how the app writes cover letters.
  import { open } from "@tauri-apps/plugin-dialog";
  import type { Config } from "../../lib/types";

  interface Props {
    config: Config;
    dataDir: string;
  }
  let { config = $bindable(), dataDir }: Props = $props();

  async function chooseFolder() {
    const dir = await open({ directory: true, multiple: false, title: "Folder for the applications" });
    if (typeof dir === "string") config.applications_dir = dir;
  }
</script>

<div class="tab">
  <h2>Applications</h2>
  <p class="hint">
    "Write application" in the job details writes a cover letter as a LaTeX file (cover_letter.tex), so that you can
    change it easily. Each letter gets its own folder, with a copy of the job posting (job.txt). An existing folder is
    never overwritten.
  </p>
  <p class="hint">
    Your data (name, address, phone, email), fixed enclosures and your own LaTeX layout go into letter.yaml in the data
    folder{dataDir ? ` (${dataDir})` : ""}. Template: letter.example.yaml in the repository.
  </p>

  <div class="row">
    <label class="field grow">
      <span>Folder for the applications</span>
      <input type="text" bind:value={config.applications_dir} placeholder="applications" spellcheck="false" />
    </label>
    <button class="btn" onclick={chooseFolder}>Choose ...</button>
  </div>
  <p class="hint">
    A relative path starts in the data folder{dataDir ? ` (${dataDir})` : ""}. ~ is your home folder.
  </p>

  <label class="field">
    <span>Instructions for the cover letter</span>
    <textarea
      bind:value={config.application_instructions}
      rows="6"
      placeholder={"For example:\nI can start on 1 March.\nMention my Rust experience first.\nWrite the letter in English."}
    ></textarea>
    <span class="hint">
      Write normal sentences. Without an instruction, the letter is in the language of the job posting. The AI uses
      only facts from your CV.
    </span>
  </label>
</div>

<style>
  .tab {
    display: flex;
    flex-direction: column;
    gap: 18px;
    max-width: 760px;
  }

  .row {
    display: flex;
    align-items: flex-end;
    gap: 8px;
    margin-bottom: -10px;
  }

  .grow {
    flex: 1;
  }
</style>
