#let files = state("kiwi-files", ())

#let graph(body) = {
    context {
        if ("x-preview" in sys.inputs) {
            body
        } else {
            if files.get().len() <= 0 {
                body
            } else {
                show html.elem: it => none
                body
            }
        }
    }
}

#let page(body) = {
    set heading(numbering: "1.", outlined: false)
    show ref: it => {
        let files = files.at(it.target)
        if files.len() > 0 {
            html.a(href: files.last().replace(".typ", ".html"), it.element.body)
        } else {
            it
        }
    }
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
        "{\n" +
        "let files = state(\"kiwi-files\", ())\n" +
        content.join(" + ") + "\n" +
        "files.update(it => it.slice(0, -1))\n" +
        "}"
    )
}
