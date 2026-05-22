#let page(body) = if ("x-preview" not in sys.inputs) {
    let h = html
    let title = state("kiwi-title", none)
    let this-file = state("kiwi-this-file", none)

    set heading(numbering: "1.", outlined: false, offset: 100)
    set document(title: context title.at(heading.where(level: 1)))

    show heading.where(offset: 100): it => {
        if it.depth == 1 {
            title.update(it.body.text)
        } else {
            heading(depth: it.depth - 1, offset: 0, outlined: true, it.body)
        }
    }

    show ref: it => {
        let target-file = this-file.at(it.target)
        if (target-file == none) {
            it
        } else {
            html.a(href: target-file.replace(".typ", ".html") + "#" + str(it.target), it.element.body)
        }
    }


    h.html[
        #h.head[
            #h.meta(charset: "utf-8")
            #h.title(context title.final())
            #h.link(rel: "icon", href: "data:image/svg+xml," + read("favicon.svg"))
            #h.style(read("index.css"))
        ]
        #h.body[
            #h.header[
                #h.h1(context title.final())
            ]
            #h.hr()
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
    set heading(numbering: "1.", outlined: false)
    body
}

#let backlinks(..links) = {
    let content = for link in links.pos() {
        (
            "this-file.update(\"" + link + "\") + " +
            "include(\"" + link + "\")",
        )
    }

    (
        "{\n" +
        "set heading(outlined: false)\n" +
        if ("x-preview" not in sys.inputs) { "show html.elem: it => {}\n" }  +
        "let this-file = state(\"kiwi-this-file\", none)\n" +
        content.join(" + ") + "\n" +
        "this-file.update(none)\n" +
        "}"
    )
}
