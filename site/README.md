# Project page

`index.html` and `style.css` are the page at https://adriaanboshoff.github.io/OpenMailClient/,
served from the `gh-pages` branch, which holds these two files, the screenshots from
`docs/images/` under `images/` (each has its row in `ATTRIBUTION.md`, copied beside them), and
an empty `.nojekyll`. To publish a change, copy them onto that branch and push it:

```bash
git worktree add ../omc-pages gh-pages
cp site/index.html site/style.css ATTRIBUTION.md ../omc-pages/
cp docs/images/*.png ../omc-pages/images/
git -C ../omc-pages commit -am "Project page brought up to date"
git -C ../omc-pages push origin gh-pages
git worktree remove ../omc-pages
```
