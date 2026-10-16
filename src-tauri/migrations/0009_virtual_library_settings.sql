INSERT INTO app_settings (key, value)
VALUES
    ('virtual_library_enabled', 'false'),
    ('virtual_library_path', '%USERPROFILE%\Pictures\Lymic')
ON CONFLICT(key) DO NOTHING;
