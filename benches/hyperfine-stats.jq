.results[0]
| (.summary.time_wall_clock // .)
| [.mean, (.stddev // 0), .min, .max, (.count // (.times | length))]
| if all(.[]; type == "number") and .[4] > 0 then @tsv
  else error("Hyperfine export has no wall-clock measurements")
  end
