#include "linuxlauncher.h"

#include <array>
#include <cerrno>
#include <cctype>
#include <cstdlib>
#include <cstring>
#include <string_view>

#include <sys/stat.h>
#include <unistd.h>

namespace {

constexpr std::string_view kEnvironmentPrefix = "--fm-env=";

constexpr std::string_view kForwardedVariables[] = {
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "XDG_RUNTIME_DIR",
    "XDG_SESSION_TYPE",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_DESKTOP",
    "DBUS_SESSION_BUS_ADDRESS",
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "QT_QPA_PLATFORM",
    "QT_QPA_PLATFORMTHEME",
    "QT_STYLE_OVERRIDE",
    "QT_SCALE_FACTOR",
    "QT_SCREEN_SCALE_FACTORS",
    "QT_FONT_DPI",
    "QT_ENABLE_HIGHDPI_SCALING",
    "QT_AUTO_SCREEN_SCALE_FACTOR",
    "QT_IM_MODULE",
    "XMODIFIERS",
    "GTK_IM_MODULE",
};

bool isForwardedVariable(std::string_view name)
{
    for (const std::string_view variable : kForwardedVariables) {
        if (variable == name) {
            return true;
        }
    }
    return false;
}

bool nonEmpty(const char *value)
{
    return value != nullptr && *value != '\0';
}

std::string trustedPkexecPath()
{
    static constexpr const char *const candidates[] = {
        "/usr/bin/pkexec",
        "/usr/sbin/pkexec",
        "/bin/pkexec",
    };
    for (const char *candidate : candidates) {
        struct stat info {};
        if (::stat(candidate, &info) != 0 || !S_ISREG(info.st_mode)) {
            continue;
        }
        // The helper must be root-owned and not writable by group or others.
        if (info.st_uid != 0 || (info.st_mode & (S_IWGRP | S_IWOTH)) != 0) {
            continue;
        }
        if (::access(candidate, X_OK) == 0) {
            return candidate;
        }
    }
    return {};
}

bool validEnvironmentName(const std::string &name)
{
    if (name.empty() || std::isdigit(static_cast<unsigned char>(name.front())) != 0) {
        return false;
    }
    for (const char character : name) {
        if (std::isalnum(static_cast<unsigned char>(character)) == 0 && character != '_') {
            return false;
        }
    }
    return true;
}

std::string shellQuote(const std::string &value)
{
    if (value.empty()) {
        return "''";
    }
    bool simple = true;
    for (const char character : value) {
        if (std::isalnum(static_cast<unsigned char>(character)) == 0
            && std::strchr("@%_-+=:,./", character) == nullptr) {
            simple = false;
            break;
        }
    }
    if (simple) {
        return value;
    }

    std::string quoted = "'";
    for (const char character : value) {
        if (character == '\'') {
            quoted += "'\\''";
        } else {
            quoted += character;
        }
    }
    quoted += '\'';
    return quoted;
}

} // namespace

namespace LinuxLauncher {

bool isElevated()
{
    return ::geteuid() == 0;
}

bool elevationDisabledByEnvironment()
{
    const char *value = std::getenv("FM_EXPLORE_NO_ELEVATE");
    return nonEmpty(value) && std::strcmp(value, "0") != 0;
}

bool hasGraphicalSession()
{
    return nonEmpty(std::getenv("DISPLAY")) || nonEmpty(std::getenv("WAYLAND_DISPLAY"));
}

std::string selfExecutable()
{
    std::array<char, 4096> buffer {};
    const ssize_t length = ::readlink("/proc/self/exe", buffer.data(), buffer.size() - 1);
    if (length <= 0) {
        return {};
    }
    return std::string(buffer.data(), static_cast<std::size_t>(length));
}

bool pkexecAvailable()
{
    return !trustedPkexecPath().empty();
}

std::vector<std::string> applyForwardedEnvironment(
    const std::vector<std::string> &arguments, bool &elevatedChild)
{
    std::vector<std::string> filtered;
    filtered.reserve(arguments.size());

    for (const std::string &argument : arguments) {
        if (argument.compare(0, kEnvironmentPrefix.size(), kEnvironmentPrefix) == 0) {
            const std::string assignment = argument.substr(kEnvironmentPrefix.size());
            const std::size_t separator = assignment.find('=');
            if (separator != std::string::npos && separator > 0) {
                const std::string name = assignment.substr(0, separator);
                const std::string value = assignment.substr(separator + 1);
                if (validEnvironmentName(name) && isForwardedVariable(name)) {
                    ::setenv(name.c_str(), value.c_str(), 1);
                }
            }
            continue;
        }
        if (argument == "--fm-elevated") {
            elevatedChild = true;
            continue;
        }
        filtered.push_back(argument);
    }
    return filtered;
}

std::vector<std::string> buildElevationCommand(
    const std::string &selfPath, const std::vector<std::string> &arguments)
{
    std::vector<std::string> command;
    command.reserve(arguments.size() + 16);
    const std::string pkexec = trustedPkexecPath();
    command.push_back(pkexec.empty() ? std::string("pkexec") : pkexec);
    command.push_back(selfPath);
    command.emplace_back("--fm-elevated");

    auto forward = [&command](const char *name, const char *value) {
        if (nonEmpty(value)) {
            command.push_back(std::string("--fm-env=") + name + "=" + value);
        }
    };

    // pkexec deliberately strips most of the caller's environment. These
    // variables are needed to attach the root process to the user's display
    // server and input method. Library and plugin search paths are not
    // forwarded because they would let an unprivileged caller inject code into
    // the root process.
    for (const std::string_view name : kForwardedVariables) {
        const std::string key(name);
        forward(key.c_str(), std::getenv(key.c_str()));
    }

    if (!nonEmpty(std::getenv("XAUTHORITY"))) {
        if (const char *home = std::getenv("HOME"); nonEmpty(home)) {
            const std::string candidate = std::string(home) + "/.Xauthority";
            if (::access(candidate.c_str(), R_OK) == 0) {
                command.push_back("--fm-env=XAUTHORITY=" + candidate);
            }
        }
    }

    for (const std::string &argument : arguments) {
        if (argument == "--no-elevate" || argument == "--print-elevation") {
            continue;
        }
        command.push_back(argument);
    }
    return command;
}

std::string describeCommand(const std::vector<std::string> &command)
{
    std::string description;
    for (const std::string &argument : command) {
        if (!description.empty()) {
            description += ' ';
        }
        description += shellQuote(argument);
    }
    return description;
}

int execCommand(const std::vector<std::string> &command)
{
    if (command.empty()) {
        return EINVAL;
    }

    std::vector<char *> arguments;
    arguments.reserve(command.size() + 1);
    for (const std::string &argument : command) {
        arguments.push_back(const_cast<char *>(argument.c_str()));
    }
    arguments.push_back(nullptr);

    ::execvp(arguments.front(), arguments.data());
    return errno != 0 ? errno : EINVAL;
}

} // namespace LinuxLauncher
