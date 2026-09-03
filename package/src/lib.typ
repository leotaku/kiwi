#let wiki = if "x-wiki" in sys.inputs {
    sys.inputs.x-wiki
} else {
    import "fake-wiki.typ"
    fake-wiki
}

#let plain-text(content) = {
    let flatten(it) = {
        return if type(it) == str {
            it
        } else if it == [ ] {
            " "
        } else if it.has("children") {
            it.children.map(flatten).join()
        } else if it.has("body") {
            flatten(it.body)
        } else if it.has("text") {
            flatten(it.text)
        } else if it.func() == smartquote {
            if it.double { "\"" } else { "'" }
        } else {
            panic("Cannot flatten type `" + repr(func) + "` to text")
        }
    }

    let balance-quotes(input) = {
        let graphemes = input.clusters()
        let windows = if graphemes.len() > 0 {
            ((" ", graphemes.at(0)),) + graphemes.windows(2)
        } else {
            return ""
        }

        let quote-rule(
            prev, char, nesting-stack,
            rules: ("\"": ("“", "”"), "'": ("‘", "’"))
        ) = {
            if (char not in rules) {
                return (char, nesting-stack)
            }

            let (opening, closing) = rules.at(char)
            let opened = nesting-stack.last(default: none)

            if (
                opened != char
                and prev.contains(regex("\d"))
            ) {
                let prime = ("\"": "″", "'": "′").at(char)
                (prime, nesting-stack)
            } else if (
                char == "'"
                and opened != char
                and prev.contains(regex("[\w\u{FFFC}]"))
            ) {
                ("’", nesting-stack)
            } else if (
                char == opened
                and not prev.contains(regex("[\s\n(\[{]"))
            ) {
                (closing, nesting-stack.slice(0, -1))
            } else {
                (opening, nesting-stack + (char,))
            }
        }

        let (balanced, _) = windows.fold(
            ("", ()),
            ((acc, nesting-stack), (prev, char)) => {
                let (char, nesting-stack) = quote-rule(prev, char, nesting-stack)
                (acc + char, nesting-stack)
            }
        )

        balanced
    }

    balance-quotes(flatten(content))
}

#let page(publish: true, body) = if "x-wiki" in sys.inputs {
    let h = html

    let in-path = wiki.input-of(body.children.first())
    let out-path = in-path.replace(regex(".typ$"), ".html")

    let title = state("kiwi-page-title", none)
    show heading: it => {
        h.elem("h" + str(it.level), it.body)
    }
    show heading.where(level: 1): it => {
        title.update(plain-text(it.body))
        h.h1(tabindex: 1, it.body)
    }
    set heading(numbering: (..it) => if it.pos().len() <= 1 {} else {
        numbering("1.",..it.pos().slice(1))
    })

    show ref: it => {
        if query(it.target).len() > 0 {
            return it
        }

        let elem = wiki.query-label(it.target)
        if elem.func() == heading and elem.level <= 1 {
            let document = wiki.document-at(elem.location())
            link(document.location(), elem.body)
        } else {
            link(elem.location(), elem.body)
        }
    }

    show link: it => {
        if type(it.dest) != label {
            return it
        } else if query(it.dest).len() > 0 {
            return it
        }

        let elem = wiki.query-label(it.dest)
        if elem.func() == heading and elem.level <= 1 {
            let document = wiki.document-at(elem.location())
            link(document.location(), elem.body)
        } else {
            link(elem.location(), elem.body)
        }
    }

    show h.elem.where(tag: "a"): it => {
        if "href" not in it.attrs {
            return it
        } else if it.attrs.href.starts-with(regex("https?://")) {
            return it
        }

        let (path, ..fragment) = it.attrs.href.split("#")
        let path = path.replace(
            regex("/index.html$"), "/"
        ).replace(
            regex("^index.html$"), "."
        )
        let href = (path, ..fragment).join("#")

        if href == it.attrs.href {
            return it
        }
        h.elem("a", attrs: (..it.attrs, href: href), it.body)
    }

    show image: it => [
        #let asset = wiki.read-asset(it.source, it)
        #asset
        #h.img(src: wiki.make-relative(asset.path, out-path), loading: "lazy")
    ]

    set raw(theme: none)

    show pagebreak: h.hr()

    show h.elem.where(tag: "pre"): it => {
        if "contenteditable" not in it.attrs {
            h.elem("pre", attrs: (tabindex: "-1", contenteditable: "true", spellcheck: "false", aria-readonly: "true", onbeforeinput: "event.preventDefault()"))[#it.body]
        } else {
            it
        }
    }

    let favicon = asset("favicon.svg", read("favicon.svg"))
    favicon

    let html = h.html(lang: "en")[
        #h.head[
            #h.meta(charset: "utf-8")
            #h.meta(name: "viewport", content: "width=device-width, initial-scale=1")
            #h.title[#context title.final() | digraph.me]
            #h.link(rel: "icon", type: "image/svg", href: wiki.make-relative(favicon.path, out-path))
            #h.style(read("index.css"))
        ]
        #h.body[
            #h.header[
                #let link = if out-path == "/index.html" {
                    ".."
                } else {
                    wiki.make-relative("/", out-path)
                }
                #h.a(href: link, style: "float: right; text-decoration: none")[«]
            ]
            #h.main[
                #body
            ]
            #h.hr()
            #h.footer[
                #h.div[Contact me per #h.a(href: "mailto:leo.gaskin@le0.gs", "Email")]
                #h.div[Find me on #h.a(href: "https://github.com/leotaku", "GitHub")]
                #h.div[© Leo Gaskin (2022-2026)]
            ]
        ]
    ]

    wiki.register-for-index(in-path)
    if publish {
        document(out-path, html)
    }
} else {
    show document: it => { it.body }
    show asset: it => { }

    body
}
