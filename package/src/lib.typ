#let page(body) = if "x-wiki" in sys.inputs {
    let h = html
    let wiki = sys.inputs.x-wiki

    let title = state("kiwi-page-title", none)
    show heading: it => {
        h.elem("h" + str(it.level), it.body)
    }
    show heading.where(level: 1): it => {
        title.update(it.body.text)
        it
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
        #html.img(src: asset.path)
    ]

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
