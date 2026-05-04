  * RST reader:

    + Fixed small bug in list parsing.  Previously the parser didn't
      handle properly this case:

          * - a
            - b
          * - c
            - d
    + Handle multiline cells in simple tables.
