# Records demo/demo.gif for the README.
#
# Needs: vhs (brew install vhs), miller (brew install miller), python3, and Ollama running
# with qwen3:1.7b (ollama pull qwen3:1.7b).
#
# Run from the repository root:
#   vhs demo/demo.tape
#
# The recording shows the real program call. The hidden part does what demo/run.sh does:
# it serves the demo pages locally and points job_radar to the demo config, sources and
# ignore list (so your private files are not used).

Output demo/demo.gif

Set Shell "bash"
Set FontFamily "Menlo"
Set FontSize 15
Set Width 1200
Set Height 640
Set Padding 20
Set Theme "Catppuccin Mocha"
Set TypingSpeed 45ms

# Preparation, not recorded: build, start the demo web server, load the model.
Hide
Type "export PS1='$ ' PATH=$PWD/target/release:$PATH AGENTS_CONFIG=demo/config.yaml JOB_RADAR_SOURCES=demo/sources.yaml JOB_RADAR_IGNORE=demo/ignore.yaml"
Enter
Type "cargo build --release --quiet && (python3 -m http.server 8765 --bind 127.0.0.1 --directory demo/site >/dev/null 2>&1 &) && ollama run qwen3:1.7b '' >/dev/null 2>&1; clear"
Enter
Wait@120s /^\$ ?$/
Show

Type "head -6 demo/demo_cv.txt"
Sleep 300ms
Enter
Sleep 2.5s

Type "job_radar --cv demo/demo_cv.txt --out demo/result.csv"
Sleep 300ms
Enter
Wait+Screen@120s /Total usage/
Wait@30s /^\$ ?$/
Sleep 1.5s

Type "mlr -M --icsv --opprint --barred cut -o -f match_score,title,company,location demo/result.csv"
Sleep 300ms
Enter
Wait@10s /^\$ ?$/
Sleep 7s

# Cleanup, not recorded: stop the demo web server.
Hide
Type "pkill -f 'http.server 8765'"
Enter
