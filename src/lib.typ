#let is-preview = "x-preview" in sys.inputs
#let files = state("kiwi-files", ())

#let graph(body) = {
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

    context {
        if is-preview {
            if files.get().len() <= 0 {
                body
            } else if files.get().len() <= 1 {
                show ref: it => none
                body
            }
        } else {
            show ref: it => {
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

            if files.get().len() <= 0 {
                reference-all-labels()
                body
            } else if files.get().len() <= 1 {
                show html.elem: it => none
                show ref: it => none
                body
            }
        }
    }
}

#let page(body) = {
    set heading(numbering: "1.", outlined: false)
    graph(body)
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
