"use client";

import { useCallback, useState } from "react";
import type { Session } from "../lib/api";
import { AdminDashboard } from "./admin-dashboard";
import { LoginScreen } from "./login-screen";
import { StaffDashboard } from "./staff-dashboard";
import { Workspace } from "./shared";

export function TaskBoard() {
  const [session, setSession] = useState<Session | null>(null);
  const [notice, setNotice] = useState("");

  const handleExpired = useCallback(() => {
    setSession(null);
    setNotice("Your session expired. Sign in again to continue.");
  }, []);

  if (!session) {
    return <LoginScreen notice={notice} onSession={(session) => { setNotice(""); setSession(session); }} />;
  }
  return (
    <Workspace user={session.user} onSignOut={() => setSession(null)}>
      {session.user.role === "admin"
        ? <AdminDashboard token={session.access_token} onExpired={handleExpired} />
        : <StaffDashboard token={session.access_token} onExpired={handleExpired} />}
    </Workspace>
  );
}
