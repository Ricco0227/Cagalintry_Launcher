import { Route, Routes } from "react-router";
import { Compass, Layers, Settings as SettingsIcon, User } from "lucide-react";

import { Rail } from "@/components/Rail";
import { TitleBar } from "@/components/TitleBar";
import { EmptyState, Page } from "@/components/Page";
import { Library } from "@/routes/Library";

export default function App() {
  return (
    <div className="flex h-full flex-col overflow-hidden bg-bg text-text">
      <TitleBar />
      <div className="flex min-h-0 flex-1">
        <Rail />
        <main className="flex min-w-0 flex-1 flex-col">
          <Routes>
            <Route path="/" element={<Library />} />
            <Route path="/packs" element={<Packs />} />
            <Route path="/discover" element={<Discover />} />
            <Route path="/accounts" element={<Accounts />} />
            <Route path="/settings" element={<Settings />} />
          </Routes>
        </main>
      </div>
    </div>
  );
}

function Packs() {
  return (
    <Page title="Packs" subtitle="Modpacks shared with your group">
      <EmptyState
        icon={<Layers size={24} />}
        title="Not connected to a sync server"
        description="Once the sync server is running, packs you and your friends publish will appear here."
      />
    </Page>
  );
}

function Discover() {
  return (
    <Page title="Discover" subtitle="Browse Modrinth">
      <EmptyState
        icon={<Compass size={24} />}
        title="Modrinth browsing coming soon"
        description="Search mods, resource packs and shaders, then add them to an instance or straight into a pack."
      />
    </Page>
  );
}

function Accounts() {
  return (
    <Page title="Accounts" subtitle="Cagalintry and Minecraft accounts">
      <EmptyState
        icon={<User size={24} />}
        title="No accounts yet"
        description="Sign in to your Cagalintry account, then link the Microsoft account that owns Minecraft."
      />
    </Page>
  );
}

function Settings() {
  return (
    <Page title="Settings" subtitle="Java, downloads and appearance">
      <EmptyState
        icon={<SettingsIcon size={24} />}
        title="Nothing to configure yet"
        description="Java runtime selection, memory allocation, download concurrency and the sync server address will live here."
      />
    </Page>
  );
}
