import { Navigate, Outlet, Route, Routes } from "react-router-dom";
import { useAuth } from "./auth/AuthContext";
import { Layout } from "./components/Layout";
import { ProtectedRoute } from "./components/ProtectedRoute";
import { AdminPage } from "./pages/AdminPage";
import { HomePage } from "./pages/HomePage";
import { FavoritesPage } from "./pages/FavoritesPage";
import { ItemDetailPage } from "./pages/ItemDetailPage";
import { LibraryPage } from "./pages/LibraryPage";
import { LibrariesPage } from "./pages/LibrariesPage";
import { LoginPage } from "./pages/LoginPage";
import { PlayerPage } from "./pages/PlayerPage";
import { PlaylistDetailPage } from "./pages/PlaylistDetailPage";
import { PlaylistsPage } from "./pages/PlaylistsPage";
import { ResumePage } from "./pages/ResumePage";
import { SearchPage } from "./pages/SearchPage";

function AdminRoute() {
  const { user } = useAuth();
  if (!user?.Policy?.IsAdministrator) {
    return <Navigate to="/" replace />;
  }
  return <Outlet />;
}

export default function App() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route element={<ProtectedRoute />}>
        <Route element={<Layout />}>
          <Route index element={<HomePage />} />
          <Route path="/libraries" element={<LibrariesPage />} />
          <Route path="/library/:id" element={<LibraryPage />} />
          <Route path="/items/:id" element={<ItemDetailPage />} />
          <Route path="/play/:id" element={<PlayerPage />} />
          <Route path="/resume" element={<ResumePage />} />
          <Route path="/favorites" element={<FavoritesPage />} />
          <Route path="/search" element={<SearchPage />} />
          <Route path="/playlists" element={<PlaylistsPage />} />
          <Route path="/playlists/:id" element={<PlaylistDetailPage />} />
          <Route element={<AdminRoute />}>
            <Route path="/admin" element={<AdminPage />} />
          </Route>
          <Route path="*" element={<Navigate to="/" replace />} />
        </Route>
      </Route>
    </Routes>
  );
}
