local M = {}

-- parse LS_COLORS into a table of key -> raw SGR string
local function parse_ls_colors(ls_colors)
    local result = {}
    for entry in ls_colors:gmatch('[^:]+') do
        local key, value = entry:match('^(.-)=(.+)$')
        if key and value then
            result[key] = value
        end
    end
    return result
end

local cached = nil

local function get_colors()
    if not cached then
        local ls_colors = wish.get_var('LS_COLORS') or os.getenv('LS_COLORS')
        cached = parse_ls_colors(ls_colors)
    end
    return cached
end

-- stat mode bit masks
local S_IFMT   = 0xF000
local S_IFDIR  = 0x4000
local S_IFCHR  = 0x2000
local S_IFBLK  = 0x6000
local S_IFREG  = 0x8000
local S_IFIFO  = 0x1000
local S_IFLNK  = 0xA000
local S_IFSOCK = 0xC000
local S_ISUID  = 0x800
local S_ISGID  = 0x400
local S_ISVTX  = 0x200
local S_IXUSR  = 0x40
local S_IXGRP  = 0x8
local S_IXOTH  = 0x1
local S_IWOTH  = 0x2

-- derive the LS_COLORS type key from a stat mode number
local function type_key_from_mode(mode)
    local fmt = bit.band(mode, S_IFMT)

    if fmt == S_IFDIR then
        if bit.band(mode, S_ISVTX) ~= 0 and bit.band(mode, S_IWOTH) ~= 0 then
            return 'tw'
        elseif bit.band(mode, S_ISVTX) ~= 0 then
            return 'st'
        elseif bit.band(mode, S_IWOTH) ~= 0 then
            return 'ow'
        end
        return 'di'
    elseif fmt == S_IFLNK then
        return 'ln'
    elseif fmt == S_IFIFO then
        return 'pi'
    elseif fmt == S_IFSOCK then
        return 'so'
    elseif fmt == S_IFBLK then
        return 'bd'
    elseif fmt == S_IFCHR then
        return 'cd'
    elseif fmt == S_IFREG then
        if bit.band(mode, S_ISUID) ~= 0 then
            return 'su'
        elseif bit.band(mode, S_ISGID) ~= 0 then
            return 'sg'
        elseif bit.band(mode, S_IXUSR + S_IXGRP + S_IXOTH) ~= 0 then
            return 'ex'
        end
        return true
    end
end

-- get the raw SGR string for a filename based on LS_COLORS
-- mode is an optional numeric stat mode (from stat())
function M.sgr_for(filename, mode)
    local colors = get_colors()

    -- try file type from stat mode first
    local key = type_key_from_mode(mode)
    if not key then
        return
    end
    if colors[key] then
        return colors[key]
    end

    -- try extension match
    local ext = filename:match('%.([^./]+)$')
    if ext then
        local sgr = colors['*.' .. ext] or colors['*.' .. ext:lower()]
        if sgr then
            return sgr
        end
    end

    -- try exact glob matches
    local basename = filename:match('[^/]+$') or filename
    local sgr = colors[basename]
    if sgr then
        return sgr
    end

    -- fallback to normal file
    return colors.no
end

return M
