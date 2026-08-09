// original author: ntjess
#let plain-text(it) = {
    return if type(it) == str {
        it
    } else if it == [ ] {
        " "
    } else if it.has("children") {
        it.children.map(plain-text).join()
    } else if it.has("body") {
        plain-text(it.body)
    } else if it.has("text") {
        plain-text(it.text)
    } else if it.func() == smartquote {
        if it.double { "\"" } else { "'" }
    } else {
        panic("Not sure how to handle type `" + repr(func) + "`")
    }
}

#let balance-quotes(input) = {
    let quotes = (
        "\"": ("“", "”"),
        "'": ("‘", "’"),
    )

    let graphemes = input.graphemes()
    let running-counts = graphemes.fold(
        (quotes.map(_ => 0),),
        (acc, char) => {
            let next = for quote in quotes.keys() {
                (str(quote): acc.last().at(quote) + int(char == quote),)
            }
            acc + (next,)
        }
    ).slice(1)

    for (char, counts) in graphemes.zip(running-counts) {
        if char in quotes {
            quotes.at(char).at(calc.rem(counts.at(char) + 1, 2))
        } else {
            char
        }
    }
}

#let page(body) = if "x-wiki" in sys.inputs {
    let h = html
    let wiki = sys.inputs.x-wiki

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

    let html-link = ext-ref => {
        let elem = ext-ref.element
        let link = if elem.func() == heading and elem.level <= 1 {
            ext-ref.page.html-link()
        } else {
            ext-ref.html-link()
        }
        link.replace(regex("/?index.html"), "/")
    }

    show ref: it => {
        if query(it.target).len() > 0 {
            return it
        }

        let queried = wiki.query-label(it.target)
        if queried == none {
            h.code([UNRESOLVED])
        } else if queried.func() == ext-ref {
            h.a(href: html-link(queried), queried.element.body)
        } else {
            it
        }
    }

    show link: it => {
        if type(it.dest) != label {
            return it
        } else if query(it.dest).len() > 0 {
            return it
        }

        let queried = wiki.query-label(it.dest)
        if queried == none {
            h.code([UNRESOLVED])
        } else if queried.func() == ext-ref {
            h.a(href: html-link(queried), it.body)
        } else {
            it
        }
    }

    show image: it => [
        #let asset = sys.inputs.x-wiki.read-asset(it.source, it)
        #asset
        #h.img(src: asset.path)
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

    h.html[
        #h.head[
            #h.meta(charset: "utf-8")
            #h.meta(name: "viewport", content: "width=device-width, initial-scale=1")
            #h.title[#context title.final() | digraph.me]
            #h.link(rel: "icon", type: "image/svg", href: favicon.path)
            #h.style(read("index.css"))
        ]
        #h.body[
            #h.header[
                #h.a(href: "/", style: "float: right; text-decoration: none")[«]
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
} else {
    show ref: it => {
        if query(it.target).len() > 0 { it }
    }
    show link: it => {
        if type(it.dest) != label or query(it.dest).len() > 0 { it }
    }

    body
}
