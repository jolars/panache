-- Pre-render script to extract version from Cargo.toml
-- This runs before Quarto renders the documentation

local function open_cargo_toml()
  -- Rendering can happen from nested docs directories depending on the page.
  -- Walk upward a few levels until we find the repository Cargo.toml.
  for depth = 0, 8 do
    local prefix = string.rep("../", depth)
    local path = prefix .. "Cargo.toml"
    local file = io.open(path, "r")
    if file then
      return file
    end
  end
  return nil
end

local function read_version()
  local cargo_toml = open_cargo_toml()
  if not cargo_toml then
    return "unknown"
  end

  local in_package_section = false
  for line in cargo_toml:lines() do
    if line:match("^%s*%[package%]%s*$") then
      in_package_section = true
    elseif in_package_section and line:match("^%s*%[.+%]%s*$") then
      break
    elseif in_package_section then
      local version = line:match('^%s*version%s*=%s*"(.-)"%s*$')
      if version then
        cargo_toml:close()
        return version
      end
    end
  end

  cargo_toml:close()
  return "unknown"
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
