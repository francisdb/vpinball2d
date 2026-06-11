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

-- Host helpers.
function playsound(name) __host("play_sound", name) end
function stopsound(name) __host("stop_sound", name) end
function set_flippers_enabled(enabled) __host("set_flippers_enabled", enabled) end
function store_get(key) return __host("store_get", key) end
function store_set(key, value) __host("store_set", key, value) end
function log(message) __host("log", tostring(message)) end
