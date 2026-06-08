#!/usr/bin/env ruby
# frozen_string_literal: true

# Reads a YAML document on stdin and reports whether libyaml (via Ruby Psych)
# accepts it. Prints "ok" on success or "err:<message>" on a parse/scan error.
#
# Uses Psych.unsafe_load so the verdict reflects libyaml's parse/scan stage
# (the question we care about: does the YAML *parse*), not Psych 4's safe-load
# class allowlist. This is the closest in-tree proxy for pandoc's Haskell
# `yaml`/libyaml frontmatter parser.

require 'psych'

input = $stdin.read
begin
  Psych.unsafe_load(input)
  print 'ok'
rescue StandardError => e
  msg = e.message.to_s.gsub(/\s+/, ' ').strip
  print "err:#{msg}"
end
