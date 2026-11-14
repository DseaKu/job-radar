# Records demo/demo.gif for the README: the recommended way, job_radar in Docker.
#
# Needs: vhs (brew install vhs), Docker running, python3, and Ollama running with
# qwen3:1.7b (ollama pull qwen3:1.7b).
#
# Run from the repository root:
#   vhs demo/demo.tape
#
# The hidden part copies the demo files into a temporary folder and serves the demo pages
# on this computer. The container reaches the pages and Ollama through host.docker.internal.

Output demo/demo.gif

Set Shell "bash"
Set FontFamily "Menlo"
Set FontSize 15
Set Width 1200
Set Height 620
Set Padding 20
Set Theme "Catppuccin Mocha"
Set TypingSpeed 15ms

# Preparation, not recorded.
Hide
Type "export PS1='$ ' D=$(mktemp -d) && cp demo/config.yaml demo/ignore.yaml demo/demo_cv.txt $D/ && sed 's/127.0.0.1/host.docker.internal/' demo/sources.yaml > $D/sources.yaml"
Enter
Type "(python3 -m http.server 8765 --bind 127.0.0.1 --directory demo/site >/dev/null 2>&1 &) && docker pull -q ghcr.io/dseaku/job-radar >/dev/null && ollama run qwen3:1.7b '' >/dev/null 2>&1; cd $D; clear"
Enter
Wait@180s /^\$ ?$/
Show

Sleep 500ms
Type "ls"
Sleep 200ms
Enter
Sleep 1.5s

Type "head -6 demo_cv.txt"
Sleep 200ms
Enter
Sleep 2.5s

Type `docker run --rm -v "$PWD:/work" ghcr.io/dseaku/job-radar --cv demo_cv.txt`
Sleep 200ms
Enter
Wait+Screen@120s /Total usage/
Wait@30s /^\$ ?$/
Sleep 8s

# Cleanup, not recorded: stop the demo web server, delete the temporary folder.
Hide
Type "pkill -f 'http.server 8765'; cd /; rm -rf $D"
Enter
