#pragma once

#include <string>
#include <vector>

namespace LinuxLauncher {

bool isElevated();
bool elevationDisabledByEnvironment();
bool hasGraphicalSession();
std::string selfExecutable();
bool pkexecAvailable();

// Applies --fm-env=NAME=VALUE arguments received from the elevated parent and
// removes launcher-only arguments from the returned list.
std::vector<std::string> applyForwardedEnvironment(
    const std::vector<std::string>& arguments, bool& elevatedChild);

// Builds the argument vector for pkexec. The returned vector starts with
// "pkexec" and can be passed to execCommand().
std::vector<std::string> buildElevationCommand(
    const std::string& selfPath, const std::vector<std::string>& arguments);

std::string describeCommand(const std::vector<std::string>& command);

// Replaces the current process. Returns an errno value if exec fails.
int execCommand(const std::vector<std::string>& command);

} // namespace LinuxLauncher
