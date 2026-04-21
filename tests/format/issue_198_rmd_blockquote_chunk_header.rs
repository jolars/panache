use panache::{
    config::{Config, Extensions, Flavor},
    format,
};

fn rmarkdown_config() -> Config {
    let flavor = Flavor::RMarkdown;
    Config {
        flavor,
        extensions: Extensions::for_flavor(flavor),
        ..Default::default()
    }
}

#[test]
fn issue_198_rmd_blockquote_chunk_header_should_be_idempotent() {
    let input = r#"## Show the chunk header in the output {#show-header}

The output of the bullet list at the end of the above example will be like this:

> - One bullet.
>
>   
>   ````
>   ```{r, eval=TRUE}`r ''`
>   ````
>   ```r
>   2 + 2
>   ```
>   ```
>   ## [1] 4
>   ```
>   ````
>   ```
>   ````
>
> - Another bullet.
"#;

    let output1 = format(input, Some(rmarkdown_config()), None);
    let output2 = format(&output1, Some(rmarkdown_config()), None);
    similar_asserts::assert_eq!(output1, output2, "Formatting should be idempotent");
}
