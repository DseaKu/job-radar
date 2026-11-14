# Records demo/demo.gif for the README.
#
# Needs: vhs (brew install vhs), plus everything that demo/run.sh needs
# (Ollama running with qwen3:1.7b).
#
# Run from the repository root:
#   vhs demo/demo.tape

Output demo/demo.gif

Set Shell "bash"
Set FontSize 15
Set Width 1100
Set Height 560
Set Padding 20
Set Theme "Catppuccin Mocha"
Set TypingSpeed 45ms

# Preparation, not recorded: build once and load the model, so the recording has no long wait.
Hide
Type "export PS1='$ ' && cargo build --release --quiet && ollama run qwen3:1.7b '' >/dev/null 2>&1; clear"
Enter
Wait@120s /^\$ ?$/
Show

Type "cat demo/demo_cv.txt | head -6"
Sleep 300ms
Enter
Sleep 2.5s

Type "./demo/run.sh"
Sleep 300ms
Enter

# The demo prints the result table last. Wait until the run is done.
Wait+Screen@120s /Total usage/
Wait@30s /^\$ ?$/
Sleep 6s
