# Records demo/demo.gif for the README.
#
# Needs: vhs (brew install vhs), python3, and Ollama running with qwen3:1.7b
# (ollama pull qwen3:1.7b).
#
# Run from the repository root:
#   vhs demo/demo.tape
#
# The recording shows the real program call. The hidden part does what demo/run.sh does:
# it serves the demo pages locally and points job_radar to the demo config, sources and
# ignore list (so your private files are not used). The output goes to demo/jobs.csv.

Output demo/demo.gif

Set Shell "bash"
Set FontFamily "Menlo"
Set FontSize 15
Set Width 1200
Set Height 560
Set Padding 20
Set Theme "Catppuccin Mocha"
Set TypingSpeed 15ms

# Preparation, not recorded: build, start the demo web server, load the model, go to demo/.
Hide
Type "export PS1='$ ' PATH=$PWD/target/release:$PATH AGENTS_CONFIG=$PWD/demo/config.yaml JOB_RADAR_SOURCES=$PWD/demo/sources.yaml JOB_RADAR_IGNORE=$PWD/demo/ignore.yaml"
Enter
Type "cargo build --release --quiet && (python3 -m http.server 8765 --bind 127.0.0.1 --directory demo/site >/dev/null 2>&1 &) && ollama run qwen3:1.7b '' >/dev/null 2>&1; cd demo; clear"
Enter
Wait@120s /^\$ ?$/
Show

Sleep 500ms
Type "head -6 demo_cv.txt"
Sleep 200ms
Enter
Sleep 2.5s

Type "job_radar --cv demo_cv.txt"
Sleep 200ms
Enter
Wait+Screen@120s /Total usage/
Wait@30s /^\$ ?$/
Sleep 8s

# Cleanup, not recorded: stop the demo web server.
Hide
Type "pkill -f 'http.server 8765'"
Enter
