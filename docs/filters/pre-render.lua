-- Pre-render script to extract version from Cargo.toml
-- This runs before Quarto renders the documentation

local function read_version()
  local cargo_toml = io.open("../Cargo.toml", "r")
  if not cargo_toml then
    return "unknown"
  end
  local content = cargo_toml:read("*all")
  cargo_toml:close()
  local version = content:match('version%s*=%s*"(.-)"')
  return version or "unknown"
end

-- Set the version as a Quarto metadata variable
return {
  {
    Meta = function(meta)
      local version = read_version()
      meta.version = version
      return meta
    end
  }
}
