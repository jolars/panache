# Emphasis with Nested Inline Elements

## Code Spans in Emphasis

*text `code here` end*

**text `code here` end**

***text `code here` end***

*text `code * asterisk` end*

**text `code ** asterisks` end**

## Math in Emphasis

*text $math$ end*

**text $math$ end**

*text $a * b$ end*

**text $a ** b$ end**

## Links in Emphasis

*text [link](url) end*

**text [link](url) end**

*text [link * here](url) end*

**text [link ** here](url) end**

## Images in Emphasis

*text ![alt](img.png) end*

**text ![alt](img.png) end**

## Nested Emphasis and Other Delimiters

*em ~~strike~~ text*

**strong ~~strike~~ text**

~~strike *em* text~~

~~strike **strong** text~~

*em ^super^ text*

**strong ^super^ text**

*em ~sub~ text*

**strong ~sub~ text**

## Complex Nesting

*em with `code`, [link](url), and ~~strike~~ all together*

**strong with `code`, [link](url), and ~~strike~~ all together**

## Edge Cases: Empty and Adjacent

*`code`*

**`code`**

*[link](url)*

**[link](url)**

`code *inside* code`

[link *emphasis* link](url)

## Escapes in Nested Content

*text \*not emphasis\* end*

*text `code \*` end*

*text [link \*](url) end*

## Unclosed Constructs

*text `unclosed code end*

*text [unclosed link end*

*text ~~unclosed strike end*

## Multiple Nesting Levels

***triple with `code` here***

***triple with [link](url) here***

***triple with ~~strike~~ here***
