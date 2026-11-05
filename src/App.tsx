import { useState } from "react";

import logo from "../assets/brand/arco-logo.svg";
import { closeConnection, listProfiles, openConnection } from "./api";
import SignIn from "./components/SignIn";
import { explainOffline } from "./errors";
import ClusterPicker, { type ClusterChoice } from "./screens/ClusterPicker";
import Connect from "./screens/Connect";
import SetupAws from "./screens/SetupAws";
import Welcome from "./screens/Welcome";
import Workspace from "./screens/Workspace";
import type { ConnectionHandle, Explanation, ProfileInfo, SavedConnection } from "./types";

type Screen =
  | { name: "welcome" }
  | { name: "setup" }
  | { name: "signin"; profile: ProfileInfo; then: Screen }
  | { name: "clusters"; profile: ProfileInfo }
  | { name: "connect"; profile: ProfileInfo; cluster: ClusterChoice }
  | { name: "connected"; handle: ConnectionHandle; saved: SavedConnection };

function App() {
  const [screen, setScreen] = useState<Screen>({ name: "welcome" });
  const [problem, setProblem] = useState<Explanation | null>(null);
  // Bumped after a sign-in so the cluster list scans again.
  const [attempt, setAttempt] = useState(0);

  async function reconnect(saved: SavedConnection) {
    setProblem(null);
    try {
      const handle = await openConnection(saved);
      setScreen({ name: "connected", handle, saved });
    } catch (err) {
      setProblem(await explainOffline(saved, err));
    }
  }

  async function afterSetup(profileName: string) {
    const profile = (await listProfiles()).find((p) => p.name === profileName);
    setScreen(profile ? { name: "clusters", profile } : { name: "welcome" });
  }

  if (screen.name === "connected") {
    return (
      <Workspace
        handle={screen.handle}
        saved={screen.saved}
        onDisconnect={() => {
          closeConnection(screen.handle.id).catch(() => {});
          setScreen({ name: "welcome" });
        }}
      />
    );
  }

  return (
    <div className="app">
      <header className="top">
        <img src={logo} alt="" width={28} height={28} />
        <h1>Arco DSQL Studio</h1>
      </header>
      <main>
        {screen.name === "welcome" && (
          <Welcome
            onPickProfile={(profile) => setScreen({ name: "clusters", profile })}
            onSignIn={(profile) => setScreen({ name: "signin", profile, then: { name: "clusters", profile } })}
            onSetup={() => setScreen({ name: "setup" })}
            onReconnect={reconnect}
            problem={problem}
          />
        )}
        {screen.name === "setup" && (
          <SetupAws onDone={afterSetup} onCancel={() => setScreen({ name: "welcome" })} />
        )}
        {screen.name === "signin" && (
          <SignIn
            profile={screen.profile}
            onDone={() => {
              setAttempt((n) => n + 1);
              setScreen(screen.then);
            }}
            onCancel={() => setScreen({ name: "welcome" })}
          />
        )}
        {screen.name === "clusters" && (
          <ClusterPicker
            key={`${screen.profile.name}-${attempt}`}
            profile={screen.profile}
            onPick={(cluster) => setScreen({ name: "connect", profile: screen.profile, cluster })}
            onBack={() => setScreen({ name: "welcome" })}
            onSignIn={() =>
              setScreen({ name: "signin", profile: screen.profile, then: { name: "clusters", profile: screen.profile } })
            }
          />
        )}
        {screen.name === "connect" && (
          <Connect
            profile={screen.profile.name}
            cluster={screen.cluster}
            onConnected={(handle, saved) => setScreen({ name: "connected", handle, saved })}
            onBack={() => setScreen({ name: "clusters", profile: screen.profile })}
          />
        )}
      </main>
    </div>
  );
}

export default App;
