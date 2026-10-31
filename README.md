# kiwi

<img src=".github/kiwi.jpg" alt="" width="400px"></img>

Kiwi is a multi-page website generator using [Typst](https://typst.app/docs/) as a templating language.

## Usage

You can compile your multi-page site by running `kiwi compile input-folder -o output-folder`.
For data safety reasons, Kiwi will not clear the contents of `output-folder`, but outputs can overwrite any existing files.

If you want to work on your site interactively, run `kiwi watch input-folder`.
This will start a local web server, as well as process that continiously monitors the contents of `input-folder`, reloading the web view whenever a file is modified.
This process will also watch the files of any imported package.
When given the `--lsp-index` flag, it can also generate a virtual "main file" that can be used with [Tinymist](https://myriad-dreamin.github.io/tinymist/) to get an improved editing experience.

Mostly, you can just treat Kiwi as a souped-up [Typst bundle mode](https://typst.app/docs/reference/bundle/) which automatically compiles all files in the input directory.
One relevant difference is that you can also create assets from within documents. Recursive documents are however not supported.
Kiwi also defines some extensions to the Typst library accessible under `sys.inputs.x-wiki`.
The available routines are listed [here](package/src/wiki-polyfill.typ).

## FAQ

### Why should I use this instead of other static-site generators?

Almost all advantages of Kiwi as a static-site generator come from the fact that it uses Typst as its templating language.
For example, Typst provides HTML generation, math typesetting with MathML, automatic source code highlighting, citation handling, etc.
Interesting site features like backlinks, tags, and taxonomies are also easy to implement on top of the Typst query language.

Kiwi also inherits incremental compilation from Typst, ensuring the `watch` mode is responsive even for big sites.

### Why should I use this instead of Typst in bundle mode?

The Kiwi multi-page site generator aims to be a thin wrapper around Typst.
Ideally, I would just use raw Typst, however at the time of writing this there are two missing features that made me create this project:

1. Automatic file discovery
2. Separate intra-document and inter-document queries

Regarding (1.) I personally think a static site generator needs support for automatic file discovery. Typst only supports manually listing all of your input files, which is a good design decision for self-contained documents, but not for multi-page sites. As such, Kiwi will automatically includes any Typst files that are contained in the input directory you point it at.

Now for (2.), i think that, for a multi-page site, most things like heading numberings, bibliographies, footnotes, ... should be contained to one page.
However, it should also be possible to query contents of other pages, e.g. to generate an index page.
Typst currently does not have an ideal solution for this. However, they are clearly [thinking about it](https://github.com/typst/typst/issues/7735#issuecomment-3908841853) and I am sure that a good upstream solution will come at some point.

So for now, with Kiwi all state, counters and queries are scoped to the current document. For inter-document queries, the functions `sys.inputs.x-wiki.query` and `sys.inputs.x-wiki.query-label` can be used.

### Is this project stable?

Absolutely not. Changes to the public API might be made at any time.

## License

Kiwi is free and open source software distributed under the terms of either the MIT or the Apache 2.0 license, at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
