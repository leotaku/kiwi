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

    show metadata: it => {
        if type(it.value) != ext-ref {
            return it
        }

        [Yay!]
    }

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
}
