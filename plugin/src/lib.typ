#let files = state("kiwi-files", ())

#let graph(body, fn: (body) => body) = {
    let reference-all-labels() = {
        show html.elem: it => none
        let labelled = query(selector.or(
            heading,
            figure,
            math.equation,
        )).filter(it => it.has("label"))

        for elem in labelled [
            #ref(elem.label)
        ]
    }
    let rewrite-links = it => {
        let files = files.at(it.target)
        if files.len() > 0 {
            html.a(
                href: files.last().replace(regex(".typ$"), ".html").replace(regex("/index.html$"), "/"),
                it.element.body
            )
        } else {
            it
        }
    }

    context {
        if "x-preview" in sys.inputs {
            if files.get().len() <= 0 {
                body
            } else if files.get().len() <= 1 {
                show ref: it => none
                body
            }
        } else {
            if files.get().len() <= 0 {
                show ref: rewrite-links
                reference-all-labels()
                fn(body)
            } else if files.get().len() <= 1 {
                show html.elem: it => none
                show ref: it => none
                body
            }
        }
    }
}

#let in-main() = files.get().len() == 0

#let page(body) = {
    let h = html

    let title = state("kiwi-page-title", none)
    show heading.where(level: 1): it => {
        if in-main() { title.update(it.body.text) }
        it
    }
    set heading(numbering: (..it) => if it.pos().len() <= 1 {} else {
        numbering("1.",..it.pos().slice(1))
    })
    show heading: it => {
        h.elem("h" + str(it.level), it.body)
    }

    graph(body, fn: body => {
        h.html[
            #h.head[
                #h.meta(charset: "utf-8")
                #h.title(context title.final())
                #h.style(read("index.css"))
            ]
            #h.body[
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
    })
}

#let backlinks(..links) = {
    let content = for link in links.pos() {
        (
            "files.update(it => it + (\"" + link + "\",)) + " +
            "include(\"" + link + "\")",
        )
    }

    (
        "let files = state(\"kiwi-files\", ())\n" +
        content.join(" + ") + "\n" +
        "files.update(it => it.slice(0, -1))\n"
    )
}
