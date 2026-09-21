Alignment comes from the separator without adding code indentation:

+-----------+
| Header    |
+==========:+
| x         |
+-----------+

Wrapped cells preserve inline constructs and literal pipes:

+----------------+---------------------+
| A              | B                   |
+================+=====================+
| a|b            | ![image](a.png)     |
|                | $x + y$             |
+----------------+---------------------+

Unicode cells retain their display-column boundaries:

+----------+----+
| 界 é 😀  | x  |
+----------+----+

Existing code indentation remains meaningful:

+-----------+
|     code  |
+-----------+

Spanning cells preserve code indentation around literal pipes:

+------------+-----+
| Header           |
+============+=====+
|     a|b    | x   |
+------------+-----+

Spanning cells keep prose indentation in aligned columns:

+------------+-----+
| Header           |
+===========:+=====+
| a|b        | x   |
+------------+-----+

Spanning cells preserve punctuation as content:

+--------+--------+
| A + B           |
+========+========+
| -      | ok     |
+--------+--------+
| :      | ok     |
+--------+--------+
| =      | ok     |
+--------+--------+

Hybrid separators keep their own fill character:

+--------+--------+
| A      | B      |
+========+========+
| text   | one    |
| =      +--------+
| more   | two    |
+--------+--------+

Joined emoji retain grid boundaries:

+------+----+
| A    | B  |
+======+====+
| 👩‍💻 | ok |
+------+----+

+--------+--------+
| 👩‍💻 + B        |
+========+========+
| 👩‍💻   | ok     |
+--------+--------+

Decomposed Hangul retains grid boundaries:

+-----+----+
| A   | B  |
+=====+====+
| 가 | ok |
+-----+----+

+--------+--------+
| 가 + B         |
+========+========+
| 가    | ok     |
+--------+--------+

Wrapping preserves literal emphasis markers:

+-----------------+
| Header          |
+=================+
| foo * bar * baz |
+-----------------+
