Multi-pipe (first wins): [[a|b|c]].

Empty body is literal: [[]].

Newline inside aborts:

[[a
b]]

Non-greedy close leaves trailing brackets: [[a]]b]].

Adjacent wikilinks: [[one]] [[two]].

Mixed with regular link [text](url) on the same line as [[wiki|link]].
