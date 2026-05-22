target := "${XDG_DATA_HOME:-$HOME/.local/share}/typst/packages/local/kiwi"

link-package:
	mkdir -p "{{target}}"
	ln -s "$(pwd)" "{{target}}/0.0.0"
