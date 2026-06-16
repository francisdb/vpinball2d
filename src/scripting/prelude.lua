-- The Lua-side half of the script bridge: builds the vpinball-style object
-- model on top of the four host entry points (__get/__set/__call_item/__host).
-- Table scripts then read naturally: `shootagain.state = LightStateOn`,
-- `nb:createball()`, `nb:kick(135, 4)`, `playsound("click")`.
--
-- Any table item can be referenced as a global by its (case-insensitive) vpx
-- name; collections are arrays of those same proxies.

-- vpinball light states.
LightStateOff = 0
LightStateOn = 1
LightStateBlinking = 2

-- Canonical key codes passed to table_keydown/table_keyup.
KeyLeftFlipper = 1
KeyRightFlipper = 2
KeyPlunger = 3
KeyStartGame = 4
KeyAddCredit = 5
KeyTiltLeft = 6
KeyTiltRight = 7
KeyTiltCenter = 8

-- Methods callable on item proxies; everything else is a property.
local METHODS = {
    createball = true,
    kick = true,
    destroyball = true,
    setvalue = true,
    addvalue = true,
    resettozero = true,
}

local proxies = {}

local function proxy_for(name)
    local p = proxies[name]
    if p then return p end
    p = setmetatable({ __name = name }, {
        __index = function(_, key)
            key = string.lower(key)
            if METHODS[key] then
                return function(_, ...)
                    return __call_item(name, key, ...)
                end
            end
            return __get(name, key)
        end,
        __newindex = function(_, key, value)
            __set(name, string.lower(key), value)
        end,
    })
    proxies[name] = p
    return p
end

-- Unknown globals resolve to item proxies (table scripts use bare item names).
setmetatable(_G, {
    __index = function(_, key)
        if type(key) == "string" and __items[string.lower(key)] then
            return proxy_for(string.lower(key))
        end
        return nil
    end,
})

-- Collections become arrays of proxies: `for _, l in ipairs(GI) do ... end`.
for cname, members in pairs(__collections) do
    local list = {}
    for i, member in ipairs(members) do
        list[i] = proxy_for(string.lower(member))
    end
    rawset(_G, cname, list)
end

-- FlexDMD scene-graph proxies. The Rust `flexdmd` module owns the scene graph;
-- these wrap actor handles so a translated script reads like the VBScript
-- original: `g = FlexDMD:NewGroup("Scene")`, `g:AddActor(img)`, `img.Bitmap = x`.

local ACTOR_METHODS = {
    addactor = true, removeactor = true, removeall = true, haschild = true,
    getimage = true, getgroup = true, getlabel = true, getframe = true,
    getvideo = true, getactor = true, setbounds = true, setposition = true,
    setsize = true, setalignedposition = true, pack = true, remove = true,
    addaction = true, clearactions = true, actionfactory = true,
}
local ACTOR_STR = { bitmap = true, text = true, name = true, font = true, src = true }
local ACTOR_BOOL = { visible = true, fillparent = true, clearbackground = true }

local fd_actor -- forward declaration

local function fd_actor_method(id, m, a, b, c, d)
    if m == "addactor" then __host("group_add", id, a.__fdid)
    elseif m == "removeactor" then __host("group_remove", id, a.__fdid)
    elseif m == "getimage" or m == "getgroup" or m == "getlabel"
        or m == "getframe" or m == "getvideo" or m == "getactor" then
        return fd_actor(__host("group_find", id, a))
    elseif m == "setbounds" then __host("actor_set_bounds", id, a, b, c, d)
    elseif m == "setposition" or m == "setalignedposition" then __host("actor_set_position", id, a, b)
    elseif m == "setsize" then __host("actor_set_size", id, a, b)
    end
    -- pack / actions / removeall are no-ops until those features land.
end

fd_actor = function(id)
    if not id then return nil end
    return setmetatable({ __fdid = id }, {
        __index = function(t, key)
            local lk = string.lower(key)
            if ACTOR_METHODS[lk] then
                return function(_, ...) return fd_actor_method(t.__fdid, lk, ...) end
            elseif ACTOR_STR[lk] then return __host("actor_get_str", t.__fdid, lk)
            elseif ACTOR_BOOL[lk] then return __host("actor_get_bool", t.__fdid, lk)
            else return __host("actor_get_num", t.__fdid, lk) end
        end,
        __newindex = function(t, key, value)
            local lk = string.lower(key)
            local vt = type(value)
            if vt == "boolean" then __host("actor_set_bool", t.__fdid, lk, value)
            elseif vt == "string" then __host("actor_set_str", t.__fdid, lk, value)
            else __host("actor_set_num", t.__fdid, lk, value) end
        end,
    })
end

local FD_METHODS = {
    newgroup = true, newimage = true, newframe = true, newlabel = true,
    lockrenderthread = true, unlockrenderthread = true,
}

local function flexdmd_proxy()
    return setmetatable({}, {
        __index = function(_, key)
            local lk = string.lower(key)
            if lk == "stage" then return fd_actor(__host("fd_get", "stage")) end
            if FD_METHODS[lk] then
                return function(_, a, b, c)
                    if lk == "newgroup" then return fd_actor(__host("fd_new_group", a))
                    elseif lk == "newimage" then return fd_actor(__host("fd_new_image", a, b))
                    elseif lk == "newframe" then return fd_actor(__host("fd_new_frame", a))
                    elseif lk == "newlabel" then return fd_actor(__host("fd_new_label", a, b, c))
                    elseif lk == "lockrenderthread" then __host("fd_lock")
                    elseif lk == "unlockrenderthread" then __host("fd_unlock") end
                end
            end
            return __host("fd_get", lk)
        end,
        __newindex = function(_, key, value)
            __host("fd_set", string.lower(key), value)
        end,
    })
end

-- vpinball-style object construction; only FlexDMD is supported.
function CreateObject(name)
    if name == "FlexDMD.FlexDMD" then return flexdmd_proxy() end
    log("CreateObject: unsupported '" .. tostring(name) .. "'")
    return nil
end

-- Host helpers.
function playsound(name) __host("play_sound", name) end
function stopsound(name) __host("stop_sound", name) end
function set_flippers_enabled(enabled) __host("set_flippers_enabled", enabled) end
function store_get(key) return __host("store_get", key) end
function store_set(key, value) __host("store_set", key, value) end
function log(message) __host("log", tostring(message)) end
