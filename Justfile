target := "${XDG_DATA_HOME:-$HOME/.local/share}/typst/packages/local/kiwi"

link-package:
	mkdir -p "{{target}}"
	ln -s "$(pwd)/package" "{{target}}/0.0.0"

unlink-package:
	rm "{{target}}/0.0.0"
