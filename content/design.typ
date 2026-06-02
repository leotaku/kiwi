#import "@local/kiwi:0.0.0": *
#show: page

= Kiwi design document and notes <design>

== Level one headings

== Backlinks

The requirement is that a link to document B in document A generates a backlink to document A inside document B. However, in the source for document B, there should be no explicit reference to document A.

=== Can this work without bundling?

The design I am envisioning is something like this, with `#make-available` being an acceptable compromise.

```typst
= A <a>
This is a link to @b.
#make-available("b")
```

```typst
= B <b>
```

But I think this cannot work without bundling, which I dislike.

=== Alternative design

```typst
= A <a>
This is a link to @b.
#forwardlinks("b")
```

```typst
= B <b>
#backlinks("a")
```

=== Actually working

```typst
#eval(backlinks("todo.typ"))
```

=== Test

@todo

// #eval(backlinks("todo.typ"))
